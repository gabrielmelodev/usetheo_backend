use std::env;

#[derive(Clone)]
pub struct Config {
    // ============================================================
    // DATABASE
    // ============================================================

    /// Banco local usado pelos dados de estudo.
    ///
    /// LOCAL:
    /// SQLITE_DATABASE_URL=sqlite://study_app.db?mode=rwc
    ///
    /// RENDER:
    /// Pode usar um SQLite local temporário no filesystem da instância.
    pub database_url: String,

    /// Banco PostgreSQL remoto usado para autenticação,
    /// sincronização e demais dados remotos.
    pub remote_database_url: String,

    // ============================================================
    // AUTH
    // ============================================================

    pub jwt_secret: String,

    pub access_token_ttl_seconds: i64,

    pub refresh_token_ttl_days: i64,

    // ============================================================
    // EMAIL / OFFLINE
    // ============================================================

    pub email_enabled: bool,

    pub offline_mode: bool,

    // ============================================================
    // SERVER
    // ============================================================

    pub host: String,

    pub port: u16,

    // ============================================================
    // RESEND
    // ============================================================

    pub resend_api_key: Option<String>,

    pub mail_from: String,

    // ============================================================
    // FRONTEND
    // ============================================================

    pub frontend_url: String,

    // ============================================================
    // MÍDIA
    // ============================================================

    pub media_storage_path: String,

    pub media_max_bytes: i64,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        // ============================================================
        // DETECTA AMBIENTE
        // ============================================================

        let is_render = env::var("RENDER")
            .map(|value| {
                value.eq_ignore_ascii_case("true")
                    || value == "1"
            })
            .unwrap_or(false);

        // ============================================================
        // SQLITE LOCAL
        // ============================================================
        //
        // IMPORTANTE:
        //
        // NÃO usamos mais DATABASE_URL para o SQLite.
        //
        // O Render normalmente fornece DATABASE_URL como PostgreSQL.
        //
        // Portanto:
        //
        // DATABASE_URL
        //     ↓
        // PostgreSQL
        //
        // SQLITE_DATABASE_URL
        //     ↓
        // SQLite
        //
        // ============================================================

        let database_url = env::var("SQLITE_DATABASE_URL")
            .unwrap_or_else(|_| {
                if is_render {
                    // O filesystem do Render não é persistente.
                    //
                    // Este banco serve apenas para manter a arquitetura
                    // atual funcionando durante a execução da instância.
                    //
                    // Os dados persistentes devem permanecer no PostgreSQL.
                    "sqlite://./storage/study_app.db?mode=rwc".to_string()
                } else {
                    // Desenvolvimento local / Theo Desktop.
                    "sqlite://study_app.db?mode=rwc".to_string()
                }
            });

        // ============================================================
        // POSTGRESQL REMOTO
        // ============================================================
        //
        // No Render, DATABASE_URL normalmente é fornecida
        // automaticamente pelo PostgreSQL conectado ao serviço.
        //
        // LOCALMENTE, podemos usar REMOTE_DATABASE_URL.
        //
        // Prioridade:
        //
        // 1. REMOTE_DATABASE_URL
        // 2. DATABASE_URL
        // 3. PostgreSQL local de desenvolvimento
        //
        // ============================================================

        let remote_database_url = env::var("REMOTE_DATABASE_URL")
            .or_else(|_| env::var("DATABASE_URL"))
            .unwrap_or_else(|_| {
                "postgres://postgres:postgres@localhost/theo_dev"
                    .to_string()
            });

        // ============================================================
        // JWT
        // ============================================================

        let jwt_secret = env::var("JWT_SECRET")
            .unwrap_or_else(|_| {
                "dev-secret-change-me-in-production".to_string()
            });

        let access_token_ttl_seconds =
            env::var("ACCESS_TOKEN_TTL_SECONDS")
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(900);

        let refresh_token_ttl_days =
            env::var("REFRESH_TOKEN_TTL_DAYS")
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(30);

        // ============================================================
        // EMAIL
        // ============================================================

        let email_enabled = env::var("EMAIL_ENABLED")
            .map(|value| {
                value.eq_ignore_ascii_case("true")
                    || value == "1"
            })
            .unwrap_or(false);

        let offline_mode = env::var("OFFLINE_MODE")
            .map(|value| {
                value.eq_ignore_ascii_case("true")
                    || value == "1"
            })
            .unwrap_or(false);

        // ============================================================
        // SERVER
        // ============================================================
        //
        // Render precisa escutar em:
        //
        // 0.0.0.0:$PORT
        //
        // Localmente continua:
        //
        // 127.0.0.1:4000
        //
        // Se HOST estiver explicitamente configurado,
        // ele sempre terá prioridade.
        //
        // ============================================================

        let host = env::var("HOST")
            .unwrap_or_else(|_| {
                if is_render {
                    "0.0.0.0".to_string()
                } else {
                    "127.0.0.1".to_string()
                }
            });

        let port = env::var("PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(4000);

        // ============================================================
        // RESEND
        // ============================================================

        let resend_api_key = env::var("RESEND_API_KEY").ok();

        let mail_from = env::var("MAIL_FROM")
            .unwrap_or_else(|_| {
                "onboarding@resend.dev".to_string()
            });

        // ============================================================
        // FRONTEND
        // ============================================================

        let frontend_url = env::var("FRONTEND_URL")
            .unwrap_or_else(|_| {
                "http://localhost:5173".to_string()
            });

        // ============================================================
        // MÍDIA
        // ============================================================

        let media_storage_path =
            env::var("MEDIA_STORAGE_PATH")
                .unwrap_or_else(|_| {
                    if is_render {
                        "./storage/media".to_string()
                    } else {
                        "./storage/media".to_string()
                    }
                });

        let media_max_bytes =
            env::var("MEDIA_MAX_BYTES")
                .ok()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(20 * 1024 * 1024);

        // ============================================================
        // LOG DE CONFIGURAÇÃO
        // ============================================================

        println!(
            "[CONFIG] Ambiente: {}",
            if is_render {
                "RENDER"
            } else {
                "LOCAL"
            }
        );

        println!("[CONFIG] Host: {}", host);
        println!("[CONFIG] Porta: {}", port);
        println!("[CONFIG] SQLite: {}", database_url);

        // NÃO imprimimos a URL do PostgreSQL,
        // pois ela contém usuário/senha.

        println!(
            "[CONFIG] PostgreSQL remoto: configurado"
        );

        Self {
            database_url,
            remote_database_url,

            jwt_secret,

            access_token_ttl_seconds,

            refresh_token_ttl_days,

            email_enabled,

            offline_mode,

            host,

            port,

            resend_api_key,

            mail_from,

            frontend_url,

            media_storage_path,

            media_max_bytes,
        }
    }
}
