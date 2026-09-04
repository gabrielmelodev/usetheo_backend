use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Goal {
    pub id: Uuid,
    pub user_id: Uuid,
    pub period: String, // daily | weekly | monthly
    pub metric: String, // minutes | questions | reviews | topics
    pub target: f64,
    pub exam_id: Option<Uuid>,
    pub active: bool,
    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
