use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Folder {
    pub id: Uuid,
    pub user_id: Uuid,
    pub parent_folder_id: Option<Uuid>,
    pub name: String,
    pub position: i32,
    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
