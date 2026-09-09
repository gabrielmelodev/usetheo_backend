use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::{note::Note, note_type::NoteType},
    state::AppState,
};
use axum::{extract::State, routing::post, Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(create))
}

#[derive(Debug, Deserialize)]
pub struct NotePayload {
    pub deck_id: Uuid,
    pub note_type_id: Uuid,
    pub fields: serde_json::Value,
    #[serde(default)]
    pub tags: Vec<String>,
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<NotePayload>,
) -> AppResult<Json<serde_json::Value>> {
    // Confirma que o deck e o note type pertencem ao usuário.
    let deck_owned: i64 = sqlx::query_scalar("select count(*) from decks where id = $1 and user_id = $2")
        .bind(payload.deck_id)
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if deck_owned == 0 {
        return Err(AppError::NotFound);
    }

    let note_type: NoteType = sqlx::query_as("select * from note_types where id = $1 and user_id = $2")
        .bind(payload.note_type_id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if !payload.fields.is_object() {
        return Err(AppError::Validation("fields deve ser um objeto".to_string()));
    }

    let mut tx = state.db.begin().await?;
    let tags_json = serde_json::to_value(&payload.tags).unwrap_or_else(|_| serde_json::json!([]));

    let note: Note = sqlx::query_as(
        "insert into notes (user_id, deck_id, note_type_id, fields, tags) values ($1, $2, $3, $4, $5) returning *",
    )
    .bind(user.id)
    .bind(payload.deck_id)
    .bind(payload.note_type_id)
    .bind(&payload.fields)
    .bind(&tags_json)
    .fetch_one(&mut *tx)
    .await?;

    let templates = note_type.templates.as_array().cloned().unwrap_or_default();
    let now = Utc::now();
    let mut card_ids = Vec::with_capacity(templates.len().max(1));

    if templates.is_empty() {
        // Note type sem templates definidos: cria ao menos um cartão "cru".
        let id: Uuid = sqlx::query_scalar(
            "insert into cards (user_id, note_id, deck_id, template_index, due) values ($1, $2, $3, 0, $4) returning id",
        )
        .bind(user.id)
        .bind(note.id)
        .bind(payload.deck_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        card_ids.push(id);
    } else {
        for (idx, _template) in templates.iter().enumerate() {
            let id: Uuid = sqlx::query_scalar(
                "insert into cards (user_id, note_id, deck_id, template_index, due) values ($1, $2, $3, $4, $5) returning id",
            )
            .bind(user.id)
            .bind(note.id)
            .bind(payload.deck_id)
            .bind(idx as i32)
            .bind(now)
            .fetch_one(&mut *tx)
            .await?;
            card_ids.push(id);
        }
    }

    tx.commit().await?;

    Ok(Json(json!({ "note": note, "card_ids": card_ids })))
}
