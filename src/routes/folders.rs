use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::folder::Folder,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::{get, patch},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index).post(create))
        .route("/reorder", patch(reorder))
        .route("/:id", get(show).put(update).delete(destroy))
        .route("/:id/move", patch(move_folder))
}

async fn index(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Folder>>> {
    let folders = sqlx::query_as::<_, Folder>(
        "select * from folders where user_id = $1 order by position, name",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(folders))
}

async fn find_owned(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Folder> {
    sqlx::query_as::<_, Folder>("select * from folders where id = $1 and user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn validate_parent(state: &AppState, user_id: Uuid, folder_id: Option<Uuid>, id: Option<Uuid>) -> AppResult<()> {
    let Some(parent_id) = folder_id else { return Ok(()); };
    if id == Some(parent_id) {
        return Err(AppError::Validation("uma pasta não pode conter a si mesma".into()));
    }
    let exists: i64 = sqlx::query_scalar("select count(*) from folders where id = $1 and user_id = $2")
        .bind(parent_id).bind(user_id).fetch_one(&state.db).await?;
    if exists == 0 { return Err(AppError::NotFound); }
    if let Some(folder_id) = id {
        let cycle: i64 = sqlx::query_scalar(
            "with recursive ancestors as (
               select id, parent_folder_id from folders where id = $1
               union all
               select f.id, f.parent_folder_id from folders f join ancestors a on f.id = a.parent_folder_id
             ) select count(*) from ancestors where id = $2",
        ).bind(parent_id).bind(folder_id).fetch_one(&state.db).await?;
        if cycle > 0 {
            return Err(AppError::Validation("não é possível mover uma pasta para dentro de sua própria árvore".into()));
        }
    }
    Ok(())
}

async fn show(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Folder>> {
    Ok(Json(find_owned(&state, user.id, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct FolderPayload {
    pub name: String,
    pub parent_folder_id: Option<Uuid>,
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<FolderPayload>,
) -> AppResult<Json<Folder>> {
    if payload.name.trim().is_empty() {
        return Err(AppError::Validation("nome é obrigatório".to_string()));
    }
    validate_parent(&state, user.id, payload.parent_folder_id, None).await?;

    let folder = sqlx::query_as::<_, Folder>(
        "insert into folders (user_id, name, parent_folder_id) values ($1, $2, $3) returning *",
    )
    .bind(user.id)
    .bind(&payload.name)
    .bind(payload.parent_folder_id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(folder))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<FolderPayload>,
) -> AppResult<Json<Folder>> {
    find_owned(&state, user.id, id).await?;
    if payload.name.trim().is_empty() {
        return Err(AppError::Validation("nome é obrigatório".to_string()));
    }
    validate_parent(&state, user.id, payload.parent_folder_id, Some(id)).await?;

    let folder = sqlx::query_as::<_, Folder>(
        "update folders set name = $1, parent_folder_id = $2, updated_at = now() where id = $3 returning *",
    )
    .bind(&payload.name)
    .bind(payload.parent_folder_id)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(folder))
}

async fn destroy(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    find_owned(&state, user.id, id).await?;
    sqlx::query("delete from folders where id = $1").bind(id).execute(&state.db).await?;
    Ok(Json(serde_json::json!({ "message": "pasta removida" })))
}

#[derive(Debug, Deserialize)]
pub struct MoveFolderPayload {
    pub parent_folder_id: Option<Uuid>,
}

async fn move_folder(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<MoveFolderPayload>,
) -> AppResult<Json<Folder>> {
    find_owned(&state, user.id, id).await?;
    validate_parent(&state, user.id, payload.parent_folder_id, Some(id)).await?;

    let folder = sqlx::query_as::<_, Folder>(
        "update folders set parent_folder_id = $1, updated_at = now() where id = $2 returning *",
    )
    .bind(payload.parent_folder_id)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(folder))
}

#[derive(Debug, Deserialize)]
pub struct ReorderPayload {
    pub ordered_ids: Vec<Uuid>,
}

async fn reorder(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ReorderPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let mut tx = state.db.begin().await?;

    for (index, folder_id) in payload.ordered_ids.iter().enumerate() {
        sqlx::query("update folders set position = $1 where id = $2 and user_id = $3")
            .bind(index as i32)
            .bind(folder_id)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(Json(serde_json::json!({ "message": "ordem atualizada" })))
}
