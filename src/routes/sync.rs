
use crate::{
    auth::middleware::CurrentUser,
    error::AppResult,
    models::{
        card::Card,
        deck::Deck,
        folder::Folder,
        note::Note,
    },
    routes::cards::render_cards,
    state::AppState,
};

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
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
    .fetch_one(&state.db)
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
    // ARQUIVOS
    // --------------------------------------------------------
    //
    // Uma única query em vez de uma query para cada tipo.
    //

    let stats = sqlx::query_as::<_, StorageStats>(
        r#"
        SELECT
            COALESCE(
                SUM(
                    CASE
                        WHEN kind = 'image'
                        THEN byte_size
                        ELSE 0
                    END
                ),
                0
            ) AS image_bytes,

            COALESCE(
                SUM(
                    CASE
                        WHEN kind = 'audio'
                        THEN byte_size
                        ELSE 0
                    END
                ),
                0
            ) AS audio_bytes,

            COUNT(*) AS file_count

        FROM media_attachments

        WHERE user_id = $1
        "#,
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

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
    .fetch_optional(&state.db)
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
    .fetch_all(&state.db)
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
            .fetch_all(&state.db)
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
    .fetch_all(&state.db)
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

    let notes_raw = sqlx::query_as::<_, Note>(
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
    .fetch_all(&state.db)
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

                "tags":
                    serde_json::to_string(&note.tags)
                        .unwrap_or_else(
                            |_| "[]".to_string()
                        ),

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
    .fetch_all(&state.db)
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

    let cards_rendered =
        render_cards(&state, cards_raw).await?;

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

    Ok(Json(json!({
        "server_time": server_time,

        "decks": decks,

        "folders": folders,

        "notes": notes,

        "cards": cards
    })))
}

