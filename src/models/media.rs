use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct MediaAttachment {
    pub id: Uuid,
    pub user_id: Uuid,
    pub deck_id: Uuid,
    pub note_id: Option<Uuid>,

    pub kind: String, // image | audio
    pub filename: String,
    pub content_type: String,
    pub byte_size: i64,
    #[serde(skip_serializing)]
    pub storage_path: String,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
}
