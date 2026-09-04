use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ActiveTimer {
    pub id: Uuid,
    pub user_id: Uuid,
    pub exam_id: Uuid,
    pub topic_id: Uuid,
    pub category: String,
    pub started_at: DateTime<Utc>,
    pub accumulated_seconds: i32,
    /// Início do trecho que está rodando agora. `None` = pausado.
    pub running_since: Option<DateTime<Utc>>,
    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
}

impl ActiveTimer {
    /// Segundos totais decorridos: o que já está "no banco" + o trecho rodando agora, se houver.
    pub fn elapsed_seconds(&self) -> i64 {
        let running_extra = match self.running_since {
            Some(since) => (Utc::now() - since).num_seconds().max(0),
            None => 0,
        };
        self.accumulated_seconds as i64 + running_extra
    }
}
