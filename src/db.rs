use sqlx::{postgres::{PgPool, PgPoolOptions}, sqlite::{SqlitePool, SqlitePoolOptions}};

/// Banco local: dados de estudo, decks, cards, editais, SRS, mídia etc.
pub async fn connect_local(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

/// Banco remoto: exclusivamente autenticação, cadastro, sessões e sincronização.
pub async fn connect_remote(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}
