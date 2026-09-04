use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::note_type::{NoteType, NoteTypeResponse},
    state::AppState,
};
use axum::{extract::State, routing::get, Json, Router};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(index).post(create))
}

async fn index(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<NoteTypeResponse>>> {
    let note_types = sqlx::query_as::<_, NoteType>(
        "select * from note_types where user_id = $1 order by name",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(note_types.into_iter().map(NoteTypeResponse::from).collect()))
}

#[derive(Debug, Deserialize)]
pub struct NoteTypePayload {
    pub name: String,
    pub fields: Vec<String>,
    pub templates: Vec<serde_json::Value>,
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<NoteTypePayload>,
) -> AppResult<Json<NoteTypeResponse>> {
    if payload.name.trim().is_empty() || payload.fields.is_empty() || payload.templates.is_empty() {
        return Err(AppError::Validation(
            "nome, ao menos um campo e ao menos um template são obrigatórios".to_string(),
        ));
    }

    let fields_json = serde_json::to_value(&payload.fields).unwrap();
    let templates_json = serde_json::to_value(&payload.templates).unwrap();

    let note_type = sqlx::query_as::<_, NoteType>(
        "insert into note_types (user_id, name, fields, templates) values ($1, $2, $3, $4) returning *",
    )
    .bind(user.id)
    .bind(&payload.name)
    .bind(&fields_json)
    .bind(&templates_json)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(NoteTypeResponse::from(note_type)))
}
