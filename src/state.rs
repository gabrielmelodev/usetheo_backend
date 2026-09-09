use crate::config::Config;
use dashmap::DashMap;
use sqlx::{PgPool, SqlitePool};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    /// Banco LOCAL. Nunca é usado para login/cadastro/sessões/sync remoto.
    pub db: SqlitePool,
    /// PostgreSQL REMOTO. Exclusivo para autenticação/cadastro/sessões/sincronização.
    pub remote_db: PgPool,
    pub config: Arc<Config>,
    pub login_attempts: Arc<DashMap<String, Vec<Instant>>>,
}
