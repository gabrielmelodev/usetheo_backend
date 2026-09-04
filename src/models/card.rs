use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Card {
    pub id: Uuid,
    pub user_id: Uuid,
    pub note_id: Uuid,
    pub deck_id: Uuid,

    pub template_index: i32,
    pub queue: String,
    pub due: DateTime<Utc>,
    pub interval_days: f64,
    pub ease_factor: f64,
    pub repetitions: i32,
    pub lapses: i32,

    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Cartão pronto para o frontend renderizar: já traz `front`/`back` calculados
/// a partir do template da note e dos campos preenchidos na note.
#[derive(Debug, Serialize)]
pub struct CardResponse {
    pub id: Uuid,
    pub note_id: Uuid,
    pub deck_id: Uuid,
    pub queue: String,
    pub due: DateTime<Utc>,
    pub interval_days: f64,
    pub ease_factor: f64,
    pub repetitions: i32,
    pub lapses: i32,
    pub front: String,
    pub back: String,
}

/// Substitui `{{NomeDoCampo}}` pelo valor correspondente em `fields`.
pub fn render_template(template: &str, fields: &serde_json::Value) -> String {
    let mut out = template.to_string();
    if let Some(obj) = fields.as_object() {
        for (key, value) in obj {
            let placeholder = format!("{{{{{key}}}}}");
            let value_str = value.as_str().map(|s| s.to_string()).unwrap_or_else(|| value.to_string());
            out = out.replace(&placeholder, &value_str);
        }
    }
    out
}
