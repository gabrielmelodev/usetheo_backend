use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub remote_database_url: String,

    pub jwt_secret: String,

    pub access_token_ttl_seconds: i64,

    pub refresh_token_ttl_days: i64,

    pub email_enabled: bool,
    pub offline_mode: bool,

    pub host: String,
    pub port: u16,

    // Resend
    pub resend_api_key: Option<String>,

    pub mail_from: String,

    pub frontend_url: String,

    // ============================================================
    // MÍDIA (armazenamento local em disco)
    // ============================================================
    pub media_storage_path: String,
    pub media_max_bytes: i64,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        Self {
            // ============================================================
            // DATABASE
            // ============================================================

            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "sqlite://study_app.db?mode=rwc".to_string()
            }),

            remote_database_url: env::var("REMOTE_DATABASE_URL").unwrap_or_else(|_| {
                "postgres://postgres:postgres@localhost/theo_dev".to_string()
            }),

            // ============================================================
            // JWT
            // ============================================================

            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| {
                    "dev-secret-change-me-in-production".to_string()
                }),

            access_token_ttl_seconds: env::var("ACCESS_TOKEN_TTL_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900), // 15 minutos

            refresh_token_ttl_days: env::var("REFRESH_TOKEN_TTL_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),

            email_enabled: env::var("EMAIL_ENABLED")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),

            offline_mode: env::var("OFFLINE_MODE")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),

            // ============================================================
            // SERVER
            // ============================================================

            host: env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),

            port: env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4000),

            // ============================================================
            // RESEND
            // ============================================================

            resend_api_key: env::var("RESEND_API_KEY").ok(),

            mail_from: env::var("MAIL_FROM")
                .unwrap_or_else(|_| {
                    "onboarding@resend.dev".to_string()
                }),

            // ============================================================
            // FRONTEND
            // ============================================================

            frontend_url: env::var("FRONTEND_URL")
                .unwrap_or_else(|_| {
                    "http://localhost:5173".to_string()
                }),

            media_storage_path: env::var("MEDIA_STORAGE_PATH")
                .unwrap_or_else(|_| "./storage/media".to_string()),

            media_max_bytes: env::var("MEDIA_MAX_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20 * 1024 * 1024), // 20 MB por arquivo
        }
    }
}