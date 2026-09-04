use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::media::MediaAttachment,
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    http::header,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/upload", post(upload))
        .route("/", get(list_for_deck))
        .route("/:id/file", get(serve_file))
        .route("/:id", delete(destroy))
}

/// Só isso conta pra armazenamento: imagem ou áudio anexado dentro de um deck.
/// Nada de texto de deck/pasta/nota entra aqui.
fn kind_for_content_type(content_type: &str) -> Option<&'static str> {
    if content_type.starts_with("image/") {
        Some("image")
    } else if content_type.starts_with("audio/") {
        Some("audio")
    } else {
        None
    }
}

async fn upload(
    State(state): State<AppState>,
    user: CurrentUser,
    mut multipart: Multipart,
) -> AppResult<Json<MediaAttachment>> {
    let mut deck_id: Option<Uuid> = None;
    let mut note_id: Option<Uuid> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename = "arquivo".to_string();
    let mut content_type = "application/octet-stream".to_string();

    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::Validation(e.to_string()))? {
        match field.name() {
            Some("deck_id") => {
                let v = field.text().await.map_err(|e| AppError::Validation(e.to_string()))?;
                deck_id = Uuid::parse_str(v.trim()).ok();
            }
            Some("note_id") => {
                let v = field.text().await.map_err(|e| AppError::Validation(e.to_string()))?;
                note_id = Uuid::parse_str(v.trim()).ok();
            }
            Some("file") => {
                filename = field.file_name().unwrap_or("arquivo").to_string();
                content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
                file_bytes = Some(field.bytes().await.map_err(|e| AppError::Validation(e.to_string()))?.to_vec());
            }
            _ => {}
        }
    }

    let deck_id = deck_id.ok_or_else(|| AppError::Validation("deck_id é obrigatório".into()))?;
    let bytes = file_bytes.ok_or_else(|| AppError::Validation("arquivo (campo 'file') é obrigatório".into()))?;

    let kind = kind_for_content_type(&content_type)
        .ok_or_else(|| AppError::Validation("só imagens e áudios são aceitos como mídia".into()))?;

    if bytes.len() as i64 > state.config.media_max_bytes {
        return Err(AppError::Validation(format!(
            "arquivo maior que o limite de {} MB",
            state.config.media_max_bytes / (1024 * 1024)
        )));
    }

    let owned: i64 = sqlx::query_scalar("select count(*) from decks where id = $1 and user_id = $2")
        .bind(deck_id)
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if owned == 0 {
        return Err(AppError::NotFound);
    }

    let attachment_id = Uuid::new_v4();
    let ext = filename.rsplit('.').next().filter(|e| e.len() <= 8).unwrap_or("bin");
    let dir = format!("{}/{}", state.config.media_storage_path, user.id);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    let storage_path = format!("{dir}/{attachment_id}.{ext}");
    std::fs::write(&storage_path, &bytes).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    let attachment = sqlx::query_as::<_, MediaAttachment>(
        r#"insert into media_attachments (id, user_id, deck_id, note_id, kind, filename, content_type, byte_size, storage_path)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
           returning *"#,
    )
    .bind(attachment_id)
    .bind(user.id)
    .bind(deck_id)
    .bind(note_id)
    .bind(kind)
    .bind(&filename)
    .bind(&content_type)
    .bind(bytes.len() as i64)
    .bind(&storage_path)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(attachment))
}

#[derive(Debug, Deserialize)]
pub struct DeckQuery {
    pub deck_id: Uuid,
}

async fn list_for_deck(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<DeckQuery>,
) -> AppResult<Json<Vec<MediaAttachment>>> {
    let attachments = sqlx::query_as::<_, MediaAttachment>(
        "select * from media_attachments where deck_id = $1 and user_id = $2 order by inserted_at desc",
    )
    .bind(q.deck_id)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(attachments))
}

async fn serve_file(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<impl IntoResponse> {
    let attachment: MediaAttachment = sqlx::query_as("select * from media_attachments where id = $1 and user_id = $2")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let bytes = std::fs::read(&attachment.storage_path).map_err(|_| AppError::NotFound)?;

    Ok((
        [(header::CONTENT_TYPE, attachment.content_type.clone())],
        Bytes::from(bytes),
    ))
}

async fn destroy(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    let attachment: MediaAttachment = sqlx::query_as("select * from media_attachments where id = $1 and user_id = $2")
        .bind(id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    sqlx::query("delete from media_attachments where id = $1").bind(id).execute(&state.db).await?;
    let _ = std::fs::remove_file(&attachment.storage_path);

    Ok(Json(serde_json::json!({ "message": "mídia removida" })))
}
