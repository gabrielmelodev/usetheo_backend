use crate::state::AppState;
use serde_json::Value;
use uuid::Uuid;

pub async fn log(
    state: &AppState,
    user_id: Option<Uuid>,
    action: &str,
    ip: Option<&str>,
    user_agent: Option<&str>,
    metadata: Value,
) {
    let result = sqlx::query(
        "insert into audit_logs (user_id, action, ip_address, user_agent, metadata) values ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(action)
    .bind(ip)
    .bind(user_agent)
    .bind(metadata)
    .execute(&state.db)
    .await;

    if let Err(e) = result {
        tracing::warn!("falha ao gravar audit log ({action}): {e:?}");
    }
}
