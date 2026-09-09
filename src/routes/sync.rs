
use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::{
        card::{render_template, Card, CardResponse},
        note_type::NoteType,
        deck::Deck,
        folder::Folder,
    },
    state::AppState,
};

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};

use chrono::{DateTime, Utc};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    time::Duration,
};
use uuid::Uuid;

// ============================================================
// ROUTER
// ============================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/version", get(version))
        .route("/storage", get(storage))
        .route("/changes", get(changes))
        .route("/watch", get(watch))
        .route("/push", post(push))
}

// ============================================================
// VERSÃO DE SINCRONIZAÇÃO
// ============================================================
//
// A versão é baseada na alteração mais recente dos dados do
// usuário.
//
// O cliente pode guardar esse valor localmente.
//
// Exemplo:
//
// servidor: 100
// cliente:  100
//
// nada mudou.
//
// servidor: 120
// cliente:  100
//
// existe algo novo para sincronizar.
//

async fn get_sync_version(
    state: &AppState,
    user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COALESCE(
            EXTRACT(
                EPOCH FROM GREATEST(
                    COALESCE(
                        (SELECT MAX(updated_at)
                         FROM decks
                         WHERE user_id = $1),
                        TIMESTAMP '1970-01-01'
                    ),

                    COALESCE(
                        (SELECT MAX(updated_at)
                         FROM notes
                         WHERE user_id = $1),
                        TIMESTAMP '1970-01-01'
                    ),

                    COALESCE(
                        (SELECT MAX(updated_at)
                         FROM cards
                         WHERE user_id = $1),
                        TIMESTAMP '1970-01-01'
                    ),

                    COALESCE(
                        (SELECT MAX(updated_at)
                         FROM folders
                         WHERE user_id = $1),
                        TIMESTAMP '1970-01-01'
                    ),

                    COALESCE(
                        (SELECT MAX(updated_at)
                         FROM exams
                         WHERE user_id = $1),
                        TIMESTAMP '1970-01-01'
                    )
                )
            )::BIGINT,
            0
        )
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.remote_db)
    .await
}

// ============================================================
// WATCH
// ============================================================
//
// Mantém uma conexão SSE aberta.
//
// O cliente não precisa ficar perguntando /version
// constantemente.
//
// O servidor verifica a versão a cada 2 segundos.
//
// IMPORTANTE:
// isso não transforma o PostgreSQL em push real.
// É um watcher simples e compatível com a estrutura atual.
//

async fn watch(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut last_version: Option<i64> = None;

        loop {
            match get_sync_version(&state, user.id).await {
                Ok(version) => {
                    if last_version != Some(version) {
                        last_version = Some(version);

                        if let Ok(event) = Event::default()
                            .event("version")
                            .json_data(json!({
                                "version": version
                            }))
                        {
                            yield Ok(event);
                        }
                    }
                }

                Err(error) => {
                    tracing::warn!(
                        user_id = %user.id,
                        error = %error,
                        "erro ao verificar versão de sincronização"
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

// ============================================================
// VERSION
// ============================================================

async fn version(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    let version = get_sync_version(&state, user.id)
        .await
        .unwrap_or(0);

    Ok(Json(json!({
        "version": version
    })))
}

// ============================================================
// STORAGE
// ============================================================

#[derive(Debug, sqlx::FromRow)]
struct StorageStats {
    image_bytes: i64,
    audio_bytes: i64,
    file_count: i64,
}

async fn storage(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Value>> {
    // --------------------------------------------------------
    // ARMAZENAMENTO DE ARQUIVOS
    // --------------------------------------------------------
    //
    // O PostgreSQL atual não possui uma tabela de mídia.
    //
    // Portanto, não consultamos "media_attachments" aqui.
    // Quando o sistema de mídia for implementado, esta parte
    // poderá ser ligada à tabela correspondente.
    //
    // Enquanto isso, o usuário possui 0 bytes de arquivos.
    //

    let stats = StorageStats {
        image_bytes: 0,
        audio_bytes: 0,
        file_count: 0,
    };

    tracing::debug!(
        user_id = %user.id,
        "storage consultado sem tabela de mídia configurada"
    );

    // --------------------------------------------------------
    // PLANO
    // --------------------------------------------------------

    let plan = sqlx::query_scalar::<_, String>(
        r#"
        SELECT COALESCE(plan, 'free')
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user.id)
    .fetch_optional(&state.remote_db)
    .await?
    .unwrap_or_else(|| "free".to_string());

    // --------------------------------------------------------
    // LIMITES
    // --------------------------------------------------------

    let limit_bytes = match plan.as_str() {
        "pro" => 5_i64 * 1024 * 1024 * 1024,
        _ => 100_i64 * 1024 * 1024,
    };

    let used_bytes = stats
        .image_bytes
        .saturating_add(stats.audio_bytes);

    let used_percentage = if limit_bytes > 0 {
        ((used_bytes as f64 / limit_bytes as f64) * 100.0)
            .min(100.0)
    } else {
        0.0
    };

    // --------------------------------------------------------
    // RESPOSTA
    // --------------------------------------------------------

    Ok(Json(json!({
        "plan": plan,
        "used_bytes": used_bytes,
        "limit_bytes": limit_bytes,
        "used_percentage": used_percentage,
        "used_formatted": format_bytes(used_bytes),
        "limit_formatted": format_bytes(limit_bytes),
        "file_count": stats.file_count,

        "breakdown": {
            "images": {
                "bytes": stats.image_bytes,
                "formatted": format_bytes(stats.image_bytes)
            },
            "audio": {
                "bytes": stats.audio_bytes,
                "formatted": format_bytes(stats.audio_bytes)
            }
        }
    })))
}
// ============================================================
// FORMAT BYTES
// ============================================================

fn format_bytes(bytes: i64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let value = bytes as f64;

    if value < MB {
        format!("{:.1} KB", value / KB)
    } else if value < GB {
        format!("{:.1} MB", value / MB)
    } else {
        format!("{:.1} GB", value / GB)
    }
}

// ============================================================
// CHANGES QUERY
// ============================================================

#[derive(Debug, Deserialize)]
pub struct ChangesQuery {
    /// Último momento sincronizado pelo cliente.
    ///
    /// Se não for enviado, o servidor retorna todos os dados
    /// existentes do usuário.
    pub since: Option<DateTime<Utc>>,
}


async fn render_cards_remote(state: &AppState, cards: Vec<Card>) -> AppResult<Vec<CardResponse>> {
    let mut notes_cache: HashMap<Uuid, RemoteNote> = HashMap::new();
    let mut note_types_cache: HashMap<Uuid, NoteType> = HashMap::new();
    let mut out = Vec::with_capacity(cards.len());

    for card in cards {
        if !notes_cache.contains_key(&card.note_id) {
            let note: RemoteNote = sqlx::query_as("select * from notes where id = $1")
                .bind(card.note_id)
                .fetch_one(&state.remote_db)
                .await?;
            notes_cache.insert(card.note_id, note);
        }
        let note = notes_cache.get(&card.note_id).unwrap();

        if !note_types_cache.contains_key(&note.note_type_id) {
            let nt: NoteType = sqlx::query_as("select * from note_types where id = $1")
                .bind(note.note_type_id)
                .fetch_one(&state.remote_db)
                .await?;
            note_types_cache.insert(note.note_type_id, nt);
        }
        let note_type = note_types_cache.get(&note.note_type_id).unwrap();
        let templates = note_type.templates.as_array().cloned().unwrap_or_default();
        let template = templates.get(card.template_index as usize);
        let (front, back) = match template {
            Some(t) => {
                let front_tpl = t.get("front_template").and_then(|v| v.as_str()).unwrap_or("");
                let back_tpl = t.get("back_template").and_then(|v| v.as_str()).unwrap_or("");
                (render_template(front_tpl, &note.fields), render_template(back_tpl, &note.fields))
            }
            None => (String::new(), String::new()),
        };
        out.push(CardResponse {
            id: card.id, note_id: card.note_id, deck_id: card.deck_id, queue: card.queue,
            due: card.due, interval_days: card.interval_days, ease_factor: card.ease_factor,
            repetitions: card.repetitions, lapses: card.lapses, front, back,
        });
    }
    Ok(out)
}

#[derive(Debug, sqlx::FromRow)]
struct RemoteNote {
    id: Uuid,
    user_id: Uuid,
    deck_id: Uuid,
    note_type_id: Uuid,
    fields: serde_json::Value,
    tags: Vec<String>,
    inserted_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
// ============================================================
// CHANGES
// ============================================================
//
// Esse endpoint é a ponte entre:
//
//     PostgreSQL
//          ↓
//      API Rust
//          ↓
//    theo-sync-core
//          ↓
//      banco local
//
// O cliente manda:
//
// /changes?since=2026-09-04T08:00:00Z
//
// e recebe somente o que mudou depois desse momento.
//
// ============================================================

async fn changes(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ChangesQuery>,
) -> AppResult<Json<Value>> {
    let since = query
        .since
        .unwrap_or(DateTime::<Utc>::MIN_UTC);

    let server_time = Utc::now();

    // ========================================================
    // DECKS
    // ========================================================

    let decks_raw = sqlx::query_as::<_, Deck>(
        r#"
        SELECT *
        FROM decks
        WHERE user_id = $1
          AND updated_at > $2
        ORDER BY updated_at ASC
        "#,
    )
    .bind(user.id)
    .bind(since)
    .fetch_all(&state.remote_db)
    .await?;

    // --------------------------------------------------------
    // IDS DOS DECKS
    // --------------------------------------------------------

    let deck_ids: Vec<Uuid> =
        decks_raw.iter().map(|deck| deck.id).collect();

    // --------------------------------------------------------
    // CONTAGEM DOS CARDS
    // --------------------------------------------------------
    //
    // Antes:
    //
    // deck 1 → SELECT COUNT
    // deck 2 → SELECT COUNT
    // deck 3 → SELECT COUNT
    // ...
    //
    // Agora:
    //
    // todos os decks → UMA query.
    //

    #[derive(Debug, sqlx::FromRow)]
    struct DeckCardCount {
        deck_id: Uuid,
        total_cards: i64,
    }

    let card_counts: HashMap<Uuid, i64> =
        if deck_ids.is_empty() {
            HashMap::new()
        } else {
            let rows = sqlx::query_as::<_, DeckCardCount>(
                r#"
                SELECT
                    deck_id,
                    COUNT(*) AS total_cards
                FROM cards
                WHERE deck_id = ANY($1)
                GROUP BY deck_id
                "#,
            )
            .bind(&deck_ids)
            .fetch_all(&state.remote_db)
            .await?;

            rows.into_iter()
                .map(|row| {
                    (
                        row.deck_id,
                        row.total_cards,
                    )
                })
                .collect()
        };

    let decks: Vec<Value> = decks_raw
        .into_iter()
        .map(|deck| {
            let total_cards =
                card_counts
                    .get(&deck.id)
                    .copied()
                    .unwrap_or(0);

            json!({
                "id": deck.id,

                "folder_id": deck.folder_id,
                "parent_deck_id": deck.parent_deck_id,

                "name": deck.name,
                "description": deck.description,

                "target_cards": deck.target_cards,

                "card_type": deck.card_type,
                "difficulty": deck.difficulty,

                "color": deck.color,
                "icon": deck.icon,

                "is_public": deck.is_public,
                "is_template": deck.is_template,

                "position": deck.position,

                "total_cards": total_cards,

                "updated_at": deck.updated_at,

                "deleted": false
            })
        })
        .collect();

    // ========================================================
    // FOLDERS
    // ========================================================

    let folders_raw = sqlx::query_as::<_, Folder>(
        r#"
        SELECT *
        FROM folders
        WHERE user_id = $1
          AND updated_at > $2
        ORDER BY updated_at ASC
        "#,
    )
    .bind(user.id)
    .bind(since)
    .fetch_all(&state.remote_db)
    .await?;

    let folders: Vec<Value> = folders_raw
        .into_iter()
        .map(|folder| {
            json!({
                "id": folder.id,

                "parent_folder_id":
                    folder.parent_folder_id,

                "name": folder.name,

                "position": folder.position,

                "updated_at": folder.updated_at,

                "deleted": false
            })
        })
        .collect();

    // ========================================================
    // NOTES
    // ========================================================

    let notes_raw = sqlx::query_as::<_, RemoteNote>(
        r#"
        SELECT *
        FROM notes
        WHERE user_id = $1
          AND updated_at > $2
        ORDER BY updated_at ASC
        "#,
    )
    .bind(user.id)
    .bind(since)
    .fetch_all(&state.remote_db)
    .await?;

    let notes: Vec<Value> = notes_raw
        .into_iter()
        .map(|note| {
            json!({
                "id": note.id,

                "deck_id": note.deck_id,

                "note_type_id":
                    note.note_type_id,

                "fields":
                    note.fields.to_string(),

                "tags": note.tags,

                "updated_at":
                    note.updated_at,

                "deleted": false
            })
        })
        .collect();

    // ========================================================
    // CARDS
    // ========================================================

    let cards_raw = sqlx::query_as::<_, Card>(
        r#"
        SELECT *
        FROM cards
        WHERE user_id = $1
          AND updated_at > $2
        ORDER BY updated_at ASC
        "#,
    )
    .bind(user.id)
    .bind(since)
    .fetch_all(&state.remote_db)
    .await?;

    // --------------------------------------------------------
    // GUARDA updated_at ANTES DO RENDER
    // --------------------------------------------------------

    let updated_at_by_card: HashMap<Uuid, DateTime<Utc>> =
        cards_raw
            .iter()
            .map(|card| {
                (
                    card.id,
                    card.updated_at,
                )
            })
            .collect();

    // --------------------------------------------------------
    // RENDERIZA CARDS
    // --------------------------------------------------------

    let cards_rendered = render_cards_remote(&state, cards_raw).await?;

    let cards: Vec<Value> = cards_rendered
        .into_iter()
        .map(|card| {
            let updated_at =
                updated_at_by_card
                    .get(&card.id)
                    .copied()
                    .unwrap_or(server_time);

            json!({
                "id": card.id,

                "note_id": card.note_id,

                "deck_id": card.deck_id,

                "queue": card.queue,

                "due": card.due,

                "interval_days":
                    card.interval_days,

                "ease_factor":
                    card.ease_factor,

                "repetitions":
                    card.repetitions,

                "lapses":
                    card.lapses,

                "front": card.front,

                "back": card.back,

                "updated_at":
                    updated_at,

                "deleted": false
            })
        })
        .collect();

    // ========================================================
    // RESPOSTA
    // ========================================================

    // ========================================================
    // EXCLUSÕES (TOMBSTONES)
    // ========================================================
    //
    // Um DELETE puro não deixa rastro para outros dispositivos
    // saberem que algo sumiu. `sync_tombstones` guarda isso.
    //

    #[derive(Debug, sqlx::FromRow)]
    struct Tombstone {
        entity_type: String,
        entity_id: Uuid,
    }

    let tombstones = sqlx::query_as::<_, Tombstone>(
        r#"
        SELECT entity_type, entity_id
        FROM sync_tombstones
        WHERE user_id = $1
          AND deleted_at > $2
        "#,
    )
    .bind(user.id)
    .bind(since)
    .fetch_all(&state.remote_db)
    .await?;

    let deletes: Vec<Value> = tombstones
        .into_iter()
        .map(|t| {
            json!({
                "entity_type": t.entity_type,
                "id": t.entity_id
            })
        })
        .collect();

    Ok(Json(json!({
        "server_time": server_time,

        "decks": decks,

        "folders": folders,

        "notes": notes,

        "cards": cards,

        "deletes": deletes
    })))
}

// ============================================================
// PUSH
// ============================================================
//
// Recebe o que foi criado/editado/apagado OFFLINE (antes de
// logar, ou enquanto sem internet) e aplica no servidor.
//
// Regra de conflito: "o mais recente vence", comparando
// `updated_at` do que chegou contra o que já existe no banco.
// Isso cobre o caso de dois dispositivos editando o mesmo
// registro sem se falarem: quem tiver o carimbo mais novo
// prevalece na próxima sincronização de cada lado.
//
// Escopo desta rota (por enquanto): decks, folders, notes,
// cards — exatamente o que `GET /sync/changes` já devolve.
// Editais, metas e cronograma manual ainda não entram aqui
// porque `/changes` também não os expõe hoje; dá pra estender
// entidade por entidade seguindo o mesmo padrão.

#[derive(Debug, Deserialize)]
struct DeleteRef {
    entity_type: String,
    id: Uuid,
}

#[derive(Debug, Deserialize)]
struct PushPayload {
    #[serde(default)]
    decks: Vec<Value>,
    #[serde(default)]
    folders: Vec<Value>,
    #[serde(default)]
    notes: Vec<Value>,
    #[serde(default)]
    cards: Vec<Value>,
    #[serde(default)]
    deletes: Vec<DeleteRef>,
}

fn req_str(v: &Value, field: &str) -> AppResult<String> {
    v.get(field)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Validation(format!("campo obrigatório ausente/ inválido: {field}")))
}

fn req_uuid(v: &Value, field: &str) -> AppResult<Uuid> {
    let s = req_str(v, field)?;
    Uuid::parse_str(&s).map_err(|_| AppError::Validation(format!("uuid inválido em {field}")))
}

fn opt_uuid(v: &Value, field: &str) -> AppResult<Option<Uuid>> {
    match v.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            Uuid::parse_str(s)
                .map(Some)
                .map_err(|_| AppError::Validation(format!("uuid inválido em {field}")))
        }
        _ => Err(AppError::Validation(format!("uuid inválido em {field}"))),
    }
}

fn req_datetime(v: &Value, field: &str) -> AppResult<DateTime<Utc>> {
    let s = req_str(v, field)?;
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| AppError::Validation(format!("data inválida em {field}")))
}

async fn push(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<PushPayload>,
) -> AppResult<Json<Value>> {
    let mut tx = state.remote_db.begin().await?;

    // --------------------------------------------------------
    // FOLDERS
    // --------------------------------------------------------

    let mut folders_applied = 0i64;
    for f in &payload.folders {
        let id = req_uuid(f, "id")?;
        let parent_folder_id = opt_uuid(f, "parent_folder_id")?;
        let name = req_str(f, "name")?;
        let position = f.get("position").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
        let updated_at = req_datetime(f, "updated_at")?;

        let result = sqlx::query(
            r#"
            INSERT INTO folders (id, user_id, parent_folder_id, name, position, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (id) DO UPDATE SET
                parent_folder_id = EXCLUDED.parent_folder_id,
                name = EXCLUDED.name,
                position = EXCLUDED.position,
                updated_at = EXCLUDED.updated_at
            WHERE folders.user_id = $2
              AND folders.updated_at <= EXCLUDED.updated_at
            "#,
        )
        .bind(id)
        .bind(user.id)
        .bind(parent_folder_id)
        .bind(&name)
        .bind(position)
        .bind(updated_at)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            folders_applied += 1;
        }
    }

    // --------------------------------------------------------
    // DECKS
    // --------------------------------------------------------
    //
    // `is_template` é ação exclusiva de admin em todo o resto do
    // sistema (ver `routes/decks.rs`) — aqui vale a mesma regra:
    // um usuário comum nunca consegue marcar um deck como modelo
    // oficial via sync, mesmo mandando `is_template: true`.

    let mut decks_applied = 0i64;
    for d in &payload.decks {
        let id = req_uuid(d, "id")?;
        let folder_id = opt_uuid(d, "folder_id")?;
        let parent_deck_id = opt_uuid(d, "parent_deck_id")?;
        let name = req_str(d, "name")?;
        let description = d.get("description").and_then(|x| x.as_str()).map(str::to_string);
        let support_text = d.get("support_text").and_then(|x| x.as_str()).map(str::to_string);
        let about = d.get("about").and_then(|x| x.as_str()).map(str::to_string);
        let target_cards = d.get("target_cards").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
        let card_type = d.get("card_type").and_then(|x| x.as_str()).unwrap_or("basic").to_string();
        let difficulty = d.get("difficulty").and_then(|x| x.as_str()).unwrap_or("medio").to_string();
        let color = d.get("color").and_then(|x| x.as_str()).unwrap_or("#556B2F").to_string();
        let icon = d.get("icon").and_then(|x| x.as_str()).map(str::to_string);
        let show_review_queue = d.get("show_review_queue").and_then(|x| x.as_bool()).unwrap_or(true);
        let allow_subdecks = d.get("allow_subdecks").and_then(|x| x.as_bool()).unwrap_or(true);
        let is_public = d.get("is_public").and_then(|x| x.as_bool()).unwrap_or(false);
        let is_template =
            d.get("is_template").and_then(|x| x.as_bool()).unwrap_or(false) && user.role == "admin";
        let position = d.get("position").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
        let copied_from_deck_id = opt_uuid(d, "copied_from_deck_id")?;
        let updated_at = req_datetime(d, "updated_at")?;

        let result = sqlx::query(
            r#"
            INSERT INTO decks (
                id, user_id, folder_id, parent_deck_id, name, description, support_text, about,
                target_cards, card_type, difficulty, color, icon,
                show_review_queue, allow_subdecks, is_public, is_official,
                anki_name, imported_from_anki, review_cards_per_day,
                is_template, position, copied_from_deck_id, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, $10, $11, $12, $13,
                $14, $15, $16, false,
                NULL, false, NULL,
                $17, $18, $19, $20
            )
            ON CONFLICT (id) DO UPDATE SET
                folder_id = EXCLUDED.folder_id,
                parent_deck_id = EXCLUDED.parent_deck_id,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                support_text = EXCLUDED.support_text,
                about = EXCLUDED.about,
                target_cards = EXCLUDED.target_cards,
                card_type = EXCLUDED.card_type,
                difficulty = EXCLUDED.difficulty,
                color = EXCLUDED.color,
                icon = EXCLUDED.icon,
                show_review_queue = EXCLUDED.show_review_queue,
                allow_subdecks = EXCLUDED.allow_subdecks,
                is_public = EXCLUDED.is_public,
                is_template = CASE WHEN $21 THEN EXCLUDED.is_template ELSE decks.is_template END,
                position = EXCLUDED.position,
                copied_from_deck_id = EXCLUDED.copied_from_deck_id,
                updated_at = EXCLUDED.updated_at
            WHERE decks.user_id = $2
              AND decks.updated_at <= EXCLUDED.updated_at
            "#,
        )
        .bind(id)
        .bind(user.id)
        .bind(folder_id)
        .bind(parent_deck_id)
        .bind(&name)
        .bind(&description)
        .bind(&support_text)
        .bind(&about)
        .bind(target_cards)
        .bind(&card_type)
        .bind(&difficulty)
        .bind(&color)
        .bind(&icon)
        .bind(show_review_queue)
        .bind(allow_subdecks)
        .bind(is_public)
        .bind(is_template)
        .bind(position)
        .bind(copied_from_deck_id)
        .bind(updated_at)
        .bind(user.role == "admin")
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            decks_applied += 1;
        }
    }

    // --------------------------------------------------------
    // NOTES
    // --------------------------------------------------------

    let mut notes_applied = 0i64;
    for n in &payload.notes {
        let id = req_uuid(n, "id")?;
        let deck_id = req_uuid(n, "deck_id")?;
        let note_type_id = req_uuid(n, "note_type_id")?;
        let fields = n.get("fields").cloned().unwrap_or_else(|| json!({}));
        if !fields.is_object() {
            return Err(AppError::Validation("notes.fields deve ser um objeto".to_string()));
        }
        let tags: Vec<String> = match n.get("tags") {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        let updated_at = req_datetime(n, "updated_at")?;

        let result = sqlx::query(
            r#"
            INSERT INTO notes (id, user_id, deck_id, note_type_id, fields, tags, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO UPDATE SET
                deck_id = EXCLUDED.deck_id,
                note_type_id = EXCLUDED.note_type_id,
                fields = EXCLUDED.fields,
                tags = EXCLUDED.tags,
                updated_at = EXCLUDED.updated_at
            WHERE notes.user_id = $2
              AND notes.updated_at <= EXCLUDED.updated_at
            "#,
        )
        .bind(id)
        .bind(user.id)
        .bind(deck_id)
        .bind(note_type_id)
        .bind(&fields)
        .bind(&tags)
        .bind(updated_at)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            notes_applied += 1;
        }
    }

    // --------------------------------------------------------
    // CARDS
    // --------------------------------------------------------
    //
    // Só os campos de estado de SRS + vínculo (note/deck) são
    // sincronizados. `template_index` não vem do cliente aqui
    // (mantém o valor existente na atualização; 0 na criação) —
    // ver aviso sobre parent_card_id/position/cloze_index na
    // resposta ao usuário: esses três campos existem no app
    // local mas não têm coluna correspondente neste schema.

    let mut cards_applied = 0i64;
    for c in &payload.cards {
        let id = req_uuid(c, "id")?;
        let note_id = req_uuid(c, "note_id")?;
        let deck_id = req_uuid(c, "deck_id")?;
        let queue = c.get("queue").and_then(|x| x.as_str()).unwrap_or("new").to_string();
        let due = req_datetime(c, "due")?;
        let interval_days = c.get("interval_days").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let ease_factor = c.get("ease_factor").and_then(|x| x.as_f64()).unwrap_or(2.5);
        let repetitions = c.get("repetitions").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
        let lapses = c.get("lapses").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
        let updated_at = req_datetime(c, "updated_at")?;

        let result = sqlx::query(
            r#"
            INSERT INTO cards (
                id, user_id, note_id, deck_id, template_index,
                queue, due, interval_days, ease_factor, repetitions, lapses, updated_at
            )
            VALUES ($1, $2, $3, $4, 0, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE SET
                note_id = EXCLUDED.note_id,
                deck_id = EXCLUDED.deck_id,
                queue = EXCLUDED.queue,
                due = EXCLUDED.due,
                interval_days = EXCLUDED.interval_days,
                ease_factor = EXCLUDED.ease_factor,
                repetitions = EXCLUDED.repetitions,
                lapses = EXCLUDED.lapses,
                updated_at = EXCLUDED.updated_at
            WHERE cards.user_id = $2
              AND cards.updated_at <= EXCLUDED.updated_at
            "#,
        )
        .bind(id)
        .bind(user.id)
        .bind(note_id)
        .bind(deck_id)
        .bind(&queue)
        .bind(due)
        .bind(interval_days)
        .bind(ease_factor)
        .bind(repetitions)
        .bind(lapses)
        .bind(updated_at)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            cards_applied += 1;
        }
    }

    // --------------------------------------------------------
    // EXCLUSÕES
    // --------------------------------------------------------

    let mut deletes_applied = 0i64;
    for del in &payload.deletes {
        let table = match del.entity_type.as_str() {
            "deck" => "decks",
            "folder" => "folders",
            "note" => "notes",
            "card" => "cards",
            other => {
                return Err(AppError::Validation(format!(
                    "entity_type de exclusão desconhecido: {other}"
                )))
            }
        };

        let query = format!("DELETE FROM {table} WHERE id = $1 AND user_id = $2");
        sqlx::query(&query)
            .bind(del.id)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO sync_tombstones (user_id, entity_type, entity_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_id, entity_type, entity_id)
            DO UPDATE SET deleted_at = now()
            "#,
        )
        .bind(user.id)
        .bind(&del.entity_type)
        .bind(del.id)
        .execute(&mut *tx)
        .await?;

        deletes_applied += 1;
    }

    tx.commit().await?;

    Ok(Json(json!({
        "applied": {
            "decks": decks_applied,
            "folders": folders_applied,
            "notes": notes_applied,
            "cards": cards_applied,
            "deletes": deletes_applied,
        },
        "server_time": Utc::now()
    })))
}

