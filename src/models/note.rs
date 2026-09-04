use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Note {
    pub id: Uuid,
    pub user_id: Uuid,
    pub deck_id: Uuid,
    pub note_type_id: Uuid,
    pub fields: serde_json::Value, // objeto {campo: valor}
    pub tags: Vec<String>,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
