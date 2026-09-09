use crate::{
    auth::middleware::{AdminUser, CurrentUser},
    error::{AppError, AppResult},
    models::{
        card::Card,
        deck::{Deck, DeckResponse},
        note::Note,
    },
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index).post(create))
        .route("/templates", get(list_templates))
        .route("/bulk-move", patch(bulk_move))
        .route("/reorder", patch(reorder))
        .route("/bulk-delete", post(bulk_delete))
        .route("/:id", get(show).put(update).delete(destroy))
        .route("/:id/move", patch(move_deck))
        .route("/:id/copy", post(copy_deck))
}

async fn total_cards(state: &AppState, deck_id: Uuid) -> AppResult<i64> {
    let count: i64 = sqlx::query_scalar("select count(*) from cards where deck_id = $1")
        .bind(deck_id)
        .fetch_one(&state.db)
        .await?;
    Ok(count)
}

pub(super) async fn ensure_template_folder(state: &AppState, user_id: Uuid) -> AppResult<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "select id from folders where user_id = $1 and name = 'Modelos do Theo' and parent_folder_id is null limit 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    {
        return Ok(id);
    }

    Ok(sqlx::query_scalar(
        "insert into folders (user_id, name) values ($1, 'Modelos do Theo') returning id",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?)
}

async fn to_response(state: &AppState, deck: Deck) -> AppResult<DeckResponse> {
    let total_cards = total_cards(state, deck.id).await?;
    Ok(DeckResponse { deck, total_cards })
}

async fn index(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<DeckResponse>>> {
    let decks = sqlx::query_as::<_, Deck>(
        "select * from decks where user_id = $1 order by position, name",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(decks.len());
    for deck in decks {
        out.push(to_response(&state, deck).await?);
    }
    Ok(Json(out))
}

/// "Baralhos do Theo": decks marcados como modelo por um admin, visíveis para
/// qualquer usuário autenticado (não precisa ser dono nem admin para ver a lista).
async fn list_templates(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<DeckResponse>>> {
    let decks = sqlx::query_as::<_, Deck>(
        "select * from decks where is_template = true order by position, name",
    )
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(decks.len());
    for deck in decks {
        out.push(to_response(&state, deck).await?);
    }
    Ok(Json(out))
}

async fn find_owned(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Deck> {
    sqlx::query_as::<_, Deck>("select * from decks where id = $1 and user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// Um deck acessível para leitura: o dono, ou qualquer deck-modelo (para poder ser copiado).
async fn find_owned_or_template(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Deck> {
    sqlx::query_as::<_, Deck>(
        "select * from decks where id = $1 and (user_id = $2 or is_template = true)",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn validate_references(
    state: &AppState,
    user_id: Uuid,
    folder_id: Option<Uuid>,
    parent_deck_id: Option<Uuid>,
    deck_id: Option<Uuid>,
) -> AppResult<()> {
    if let Some(folder_id) = folder_id {
        let count: i64 = sqlx::query_scalar("select count(*) from folders where id = $1 and user_id = $2")
            .bind(folder_id).bind(user_id).fetch_one(&state.db).await?;
        if count == 0 { return Err(AppError::NotFound); }
    }
    if let Some(parent_id) = parent_deck_id {
        if deck_id == Some(parent_id) {
            return Err(AppError::Validation("um deck não pode conter a si mesmo".into()));
        }
        let count: i64 = sqlx::query_scalar("select count(*) from decks where id = $1 and user_id = $2")
            .bind(parent_id).bind(user_id).fetch_one(&state.db).await?;
        if count == 0 { return Err(AppError::NotFound); }
    }
    Ok(())
}

async fn show(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<DeckResponse>> {
    let deck = find_owned_or_template(&state, user.id, id).await?;
    Ok(Json(to_response(&state, deck).await?))
}

#[derive(Debug, Deserialize, Default)]
pub struct DeckPayload {
    pub name: String,
    pub description: Option<String>,
    pub support_text: Option<String>,
    pub about: Option<String>,
    pub folder_id: Option<Uuid>,
    pub parent_deck_id: Option<Uuid>,
    pub target_cards: Option<i32>,
    pub card_type: Option<String>,
    pub difficulty: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub show_review_queue: Option<bool>,
    pub allow_subdecks: Option<bool>,
    pub is_public: Option<bool>,
    pub review_cards_per_day: Option<i32>,
    /// Só tem efeito se quem está criando/editando for admin (ver `is_template` abaixo).
    pub is_template: Option<bool>,
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<DeckPayload>,
) -> AppResult<Json<DeckResponse>> {
    if payload.name.trim().is_empty() {
        return Err(AppError::Validation("nome é obrigatório".to_string()));
    }
    validate_references(&state, user.id, payload.folder_id, payload.parent_deck_id, None).await?;

    // Somente admin pode marcar um deck como "Baralho do Theo" (modelo).
    let is_template = payload.is_template.unwrap_or(false) && user.role == "admin";
    let folder_id = if is_template {
        Some(ensure_template_folder(&state, user.id).await?)
    } else {
        payload.folder_id
    };

    let deck = sqlx::query_as::<_, Deck>(
        r#"insert into decks
           (user_id, name, description, support_text, about, folder_id, parent_deck_id,
            target_cards, card_type, difficulty, color, icon,
            show_review_queue, allow_subdecks, is_public, review_cards_per_day, is_template)
           values ($1, $2, $3, $4, $5, $6, $7,
                   coalesce($8, 20), coalesce($9, 'basic'), coalesce($10, 'medio'),
                   coalesce($11, '#8b5cf6'), $12,
                   coalesce($13, true), coalesce($14, true), coalesce($15, false), $16, $17)
           returning *"#,
    )
    .bind(user.id)
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(&payload.support_text)
    .bind(&payload.about)
    .bind(folder_id)
    .bind(payload.parent_deck_id)
    .bind(payload.target_cards)
    .bind(&payload.card_type)
    .bind(&payload.difficulty)
    .bind(&payload.color)
    .bind(&payload.icon)
    .bind(payload.show_review_queue)
    .bind(payload.allow_subdecks)
    .bind(payload.is_public)
    .bind(payload.review_cards_per_day)
    .bind(is_template)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(to_response(&state, deck).await?))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<DeckPayload>,
) -> AppResult<Json<DeckResponse>> {
    let existing = find_owned(&state, user.id, id).await?;
    validate_references(&state, user.id, payload.folder_id.or(existing.folder_id), payload.parent_deck_id.or(existing.parent_deck_id), Some(id)).await?;

    let is_template = match payload.is_template {
        Some(v) if user.role == "admin" => v,
        _ => existing.is_template,
    };

    let deck = sqlx::query_as::<_, Deck>(
        r#"update decks set
             name = $1, description = $2, support_text = $3, about = $4,
             folder_id = $5, parent_deck_id = $6,
             target_cards = coalesce($7, target_cards),
             card_type = coalesce($8, card_type),
             difficulty = coalesce($9, difficulty),
             color = coalesce($10, color),
             icon = $11,
             show_review_queue = coalesce($12, show_review_queue),
             allow_subdecks = coalesce($13, allow_subdecks),
             is_public = coalesce($14, is_public),
             review_cards_per_day = $15,
             is_template = $16,
             updated_at = CURRENT_TIMESTAMP
           where id = $17
           returning *"#,
    )
    .bind(if payload.name.trim().is_empty() { existing.name } else { payload.name })
    .bind(payload.description.or(existing.description))
    .bind(payload.support_text.or(existing.support_text))
    .bind(payload.about.or(existing.about))
    .bind(payload.folder_id.or(existing.folder_id))
    .bind(payload.parent_deck_id.or(existing.parent_deck_id))
    .bind(payload.target_cards)
    .bind(&payload.card_type)
    .bind(&payload.difficulty)
    .bind(&payload.color)
    .bind(payload.icon.or(existing.icon))
    .bind(payload.show_review_queue)
    .bind(payload.allow_subdecks)
    .bind(payload.is_public)
    .bind(payload.review_cards_per_day.or(existing.review_cards_per_day))
    .bind(is_template)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(to_response(&state, deck).await?))
}

async fn destroy(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    find_owned(&state, user.id, id).await?;
    sqlx::query("delete from decks where id = $1").bind(id).execute(&state.db).await?;
    Ok(Json(json!({ "message": "deck removido" })))
}

#[derive(Debug, Deserialize)]
pub struct MoveDeckPayload {
    pub folder_id: Option<Uuid>,
}

async fn move_deck(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<MoveDeckPayload>,
) -> AppResult<Json<DeckResponse>> {
    find_owned(&state, user.id, id).await?;
    validate_references(&state, user.id, payload.folder_id, None, Some(id)).await?;

    let deck = sqlx::query_as::<_, Deck>(
        "update decks set folder_id = $1, updated_at = CURRENT_TIMESTAMP where id = $2 returning *",
    )
    .bind(payload.folder_id)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(to_response(&state, deck).await?))
}

// ---------------- Organização em massa ----------------

#[derive(Debug, Deserialize)]
pub struct BulkMovePayload {
    pub deck_ids: Vec<Uuid>,
    pub folder_id: Option<Uuid>,
}

/// Move vários decks (do usuário) para uma pasta de uma vez (ou para "sem pasta", se `folder_id` for nulo).
async fn bulk_move(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<BulkMovePayload>,
) -> AppResult<Json<serde_json::Value>> {
    validate_references(&state, user.id, payload.folder_id, None, None).await?;
    let mut updated = 0i64;
    for deck_id in payload.deck_ids {
        let result = sqlx::query(
            "update decks set folder_id = $1, updated_at = CURRENT_TIMESTAMP where user_id = $2 and id = $3",
        )
        .bind(payload.folder_id)
        .bind(user.id)
        .bind(deck_id)
        .execute(&state.db)
        .await?;
        updated += result.rows_affected() as i64;
    }
    Ok(Json(json!({ "moved": updated })))
}

#[derive(Debug, Deserialize)]
pub struct ReorderPayload {
    /// Lista de deck_ids na ordem final desejada; cada um recebe sua posição (índice na lista).
    pub ordered_ids: Vec<Uuid>,
}

/// Define a posição de exibição de vários decks de uma vez (drag-and-drop no painel).
async fn reorder(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ReorderPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let mut tx = state.db.begin().await?;

    for (index, deck_id) in payload.ordered_ids.iter().enumerate() {
        sqlx::query("update decks set position = $1 where id = $2 and user_id = $3")
            .bind(index as i32)
            .bind(deck_id)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(Json(json!({ "message": "ordem atualizada" })))
}

#[derive(Debug, Deserialize)]
pub struct BulkDeletePayload {
    pub deck_ids: Vec<Uuid>,
}

async fn bulk_delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<BulkDeletePayload>,
) -> AppResult<Json<serde_json::Value>> {
    let mut deleted = 0i64;
    for deck_id in payload.deck_ids {
        let result = sqlx::query("delete from decks where user_id = $1 and id = $2")
            .bind(user.id)
            .bind(deck_id)
            .execute(&state.db)
            .await?;
        deleted += result.rows_affected() as i64;
    }
    Ok(Json(json!({ "deleted": deleted })))
}

/// Copia um baralho-modelo (ou qualquer deck visível) inteiro — notes e cards — para a
/// conta do usuário atual. As cards da cópia nascem zeradas (fila "new"), como um deck novo.
async fn copy_deck(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<DeckResponse>> {
    let source = find_owned_or_template(&state, user.id, id).await?;

    let mut tx = state.db.begin().await?;

    let source_decks: Vec<Deck> = sqlx::query_as(
                "with recursive tree as (
                     select id, 0 as depth from decks where id = $1
                     union all
                     select d.id, t.depth + 1 from decks d join tree t on d.parent_deck_id = t.id
                     where d.user_id = (select user_id from decks where id = $1)
                 ) select d.* from decks d join tree t on t.id = d.id order by t.depth, d.position, d.name",
    )
    .bind(source.id).fetch_all(&mut *tx).await?;
    let mut copied_ids = HashMap::with_capacity(source_decks.len());
    let mut copied_root = None;

    for source_deck in source_decks {
    let parent_id = source_deck.parent_deck_id.and_then(|parent| copied_ids.get(&parent).copied());
    let new_deck = sqlx::query_as::<_, Deck>(
        r#"insert into decks
           (user_id, name, description, support_text, about,
            parent_deck_id, target_cards, card_type, difficulty, color, icon,
            show_review_queue, allow_subdecks, copied_from_deck_id)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
           returning *"#,
    )
    .bind(user.id)
    .bind(&source_deck.name)
    .bind(&source_deck.description)
    .bind(&source_deck.support_text)
    .bind(&source_deck.about)
    .bind(parent_id)
    .bind(source_deck.target_cards)
    .bind(&source_deck.card_type)
    .bind(&source_deck.difficulty)
    .bind(&source_deck.color)
    .bind(&source_deck.icon)
    .bind(source_deck.show_review_queue)
    .bind(source_deck.allow_subdecks)
    .bind(source_deck.id)
    .fetch_one(&mut *tx)
    .await?;
    copied_ids.insert(source_deck.id, new_deck.id);
    if source_deck.id == source.id { copied_root = Some(new_deck); }

    let notes: Vec<Note> = sqlx::query_as("select * from notes where deck_id = $1")
        .bind(source_deck.id)
        .fetch_all(&mut *tx)
        .await?;

    for note in &notes {
        let new_note_id: Uuid = sqlx::query_scalar(
            "insert into notes (user_id, deck_id, note_type_id, fields, tags) values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(user.id)
        .bind(*copied_ids.get(&source_deck.id).unwrap())
        .bind(note.note_type_id)
        .bind(&note.fields)
        .bind(&note.tags)
        .fetch_one(&mut *tx)
        .await?;

        let cards: Vec<Card> = sqlx::query_as("select * from cards where note_id = $1")
            .bind(note.id)
            .fetch_all(&mut *tx)
            .await?;

        for card in cards {
            sqlx::query(
                "insert into cards (user_id, note_id, deck_id, template_index, due) values ($1, $2, $3, $4, CURRENT_TIMESTAMP)",
            )
            .bind(user.id)
            .bind(new_note_id)
            .bind(*copied_ids.get(&source_deck.id).unwrap())
            .bind(card.template_index)
            .execute(&mut *tx)
            .await?;
        }
    }
    }

    tx.commit().await?;

    Ok(Json(to_response(&state, copied_root.ok_or(AppError::NotFound)?).await?))
}

// ---------------- Administração (somente admin) ----------------

/// Alterna se um deck do próprio admin é um "Baralho do Theo" (modelo público).
/// Reaproveita o `update` normal por trás, mas fica explícito e restrito por `AdminUser`.
pub async fn admin_set_template(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> AppResult<Json<DeckResponse>> {
    let is_template = payload.get("is_template").and_then(|v| v.as_bool()).unwrap_or(false);
    let folder_id = if is_template {
        Some(ensure_template_folder(&state, _admin.0.id).await?)
    } else {
        None
    };

    // `is_official` é o que o catálogo de matérias (/materias) realmente lê — mantemos as
    // duas colunas sincronizadas aqui pra "Tornar modelo" no admin valer nos dois lugares.
    let deck = sqlx::query_as::<_, Deck>(
        "update decks set is_template = $1, is_official = $1, folder_id = coalesce($2, folder_id), updated_at = CURRENT_TIMESTAMP
         where id = $3 and user_id = $4 returning *",
    )
    .bind(is_template)
    .bind(folder_id)
    .bind(id)
    .bind(_admin.0.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(to_response(&state, deck).await?))
}
