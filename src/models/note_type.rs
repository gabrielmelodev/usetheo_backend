use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct NoteType {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub fields: serde_json::Value,    // array de strings
    pub templates: serde_json::Value, // array de {name, front_template, back_template}
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct NoteTypeResponse {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    /// O frontend espera essa lista como uma string já serializada em JSON.
    pub fields_json: String,
    pub templates: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<NoteType> for NoteTypeResponse {
    fn from(nt: NoteType) -> Self {
        NoteTypeResponse {
            id: nt.id,
            owner_id: nt.user_id,
            name: nt.name,
            fields_json: serde_json::to_string(&nt.fields).unwrap_or_else(|_| "[]".to_string()),
            templates: nt.templates,
            created_at: nt.inserted_at,
            updated_at: nt.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Template {
    pub name: String,
    pub front_template: String,
    pub back_template: String,
}
