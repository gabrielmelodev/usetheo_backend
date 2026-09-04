use crate::config::Config;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    // chave: "email|ip" -> lista de timestamps das tentativas recentes
    pub login_attempts: Arc<DashMap<String, Vec<Instant>>>,
}
