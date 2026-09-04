mod audit;
mod auth;
mod config;
mod db;
mod email;
mod error;
mod models;
mod rate_limit;
mod review_ladder;
mod routes;
mod srs;
mod state;
mod service;

use axum::{http::Method, Router};
use config::Config;
use dashmap::DashMap;
use state::AppState;
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "theo=info,tower_http=info".into()),
        )
        .init();

    println!("========================================");
    println!("        THEO BACKEND - INICIANDO");
    println!("========================================");

    // ============================================================
    // 1. CONFIGURAÇÃO
    // ============================================================

    println!("[1/6] Carregando configuração...");

    let config = Config::from_env();

    println!("[OK] Configuração carregada.");
    println!("[INFO] Porta: {}", config.port);

    // ============================================================
    // 2. BANCO DE DADOS
    // ============================================================

    println!("[2/6] Conectando ao PostgreSQL...");

    let db = match db::connect(&config.database_url).await {
        Ok(db) => {
            println!("[OK] PostgreSQL conectado com sucesso.");
            db
        }

        Err(error) => {
            eprintln!();
            eprintln!("========================================");
            eprintln!("      ERRO AO CONECTAR AO POSTGRESQL");
            eprintln!("========================================");
            eprintln!();
            eprintln!("O erro aconteceu antes das migrations.");
            eprintln!();
            eprintln!("Detalhes:");
            eprintln!("{error:#}");
            eprintln!();
            eprintln!("Verifique:");
            eprintln!("1. PostgreSQL está executando.");
            eprintln!("2. DATABASE_URL está correta.");
            eprintln!("3. Usuário e senha estão corretos.");
            eprintln!("4. O banco informado existe.");
            eprintln!("5. PostgreSQL está aceitando conexões.");
            eprintln!("6. lc_messages está configurado para UTF-8/C.");
            eprintln!();

            return Err(error.into());
        }
    };

    // ============================================================
    // 3. MIGRATIONS
    // ============================================================

    println!("[3/6] Executando migrations...");

    match sqlx::migrate!("./migrations").run(&db).await {
        Ok(_) => {
            println!("[OK] Migrations executadas com sucesso.");
        }

        Err(error) => {
            eprintln!();
            eprintln!("========================================");
            eprintln!("          ERRO NAS MIGRATIONS");
            eprintln!("========================================");
            eprintln!();
            eprintln!("{error:#}");
            eprintln!();

            return Err(error.into());
        }
    }

    // ============================================================
    // 4. ESTADO DA APLICAÇÃO
    // ============================================================

    println!("[4/6] Criando estado da aplicação...");

    let state = AppState {
        db,
        config: Arc::new(config.clone()),
        login_attempts: Arc::new(DashMap::new()),
    };

    println!("[OK] Estado criado.");

    // ============================================================
    // 5. ROTAS
    // ============================================================

    println!("[5/6] Configurando rotas...");

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers(Any);

    // ============================================================
    // ROTAS DE EXAMES
    // ============================================================

    let exams_router = routes::exams::router()
        .merge(routes::schedule::router())
        .merge(routes::exam_dashboard::router());

    // ============================================================
    // API
    // ============================================================

    let api = Router::new()
        // -------------------------
        // AUTENTICAÇÃO
        // -------------------------
        .nest("/auth", routes::auth::router())

        // -------------------------
        // USUÁRIOS
        // -------------------------
        .nest("/users", routes::users::router())

        // -------------------------
        // ADMIN
        // -------------------------
        .nest("/admin", routes::admin::router())

        // -------------------------
        // ORGANIZAÇÃO
        // -------------------------
        .nest("/folders", routes::folders::router())
        .nest("/decks", routes::decks::router())
        .nest("/note-types", routes::note_types::router())
        .nest("/notes", routes::notes::router())
        .nest("/cards", routes::cards::router())

        // -------------------------
        // REVISÕES
        // -------------------------
        .nest("/reviews", routes::reviews::router())
        .nest("/topic-reviews", routes::topic_reviews::router())

        // -------------------------
        // ESTATÍSTICAS
        // -------------------------
        .nest("/stats", routes::stats::router())

        // -------------------------
        // MATÉRIAS
        // -------------------------
        .nest("/materias", routes::materias::router())

        // -------------------------
        // MÍDIA
        // -------------------------
        .nest("/media", routes::media::router())

        // -------------------------
        // METAS
        // -------------------------
        .nest("/goals", routes::goals::router())

        // -------------------------
        // CRONÔMETRO
        // -------------------------
        .nest("/timer", routes::timer::router())

        // -------------------------
        // DASHBOARD
        // -------------------------
        .nest("/dashboard", routes::dashboard::router())

        // -------------------------
        // QUESTÕES
        // -------------------------
        .nest("/questions", routes::questions::router())

        // -------------------------
        // LEARNING ENGINE
        // -------------------------
        .nest(
            "/learning-engine",
            routes::learning_engine::router(),
        )

        // -------------------------
        // EXAMES
        // -------------------------
        .nest("/exams", exams_router)

        // -------------------------
        // ESTUDO
        // -------------------------
        .nest("/study-logs", routes::study_logs::router())

        // -------------------------
        // SINCRONIZAÇÃO
        // -------------------------
        .nest("/sync", routes::sync::router())

        // ========================================================
        // THEO AI
        // ========================================================
        //
        // Isso cria:
        //
        // POST /api/ai/chat
        // GET  /api/ai/recommendations
        //
        // porque este Router inteiro é montado abaixo em /api.
        //
        .nest("/ai", routes::ai::router());

    // ============================================================
    // APP PRINCIPAL
    // ============================================================

    let app = Router::new()
        .nest("/api", api)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    println!("[OK] Rotas configuradas.");

    // ============================================================
    // 6. SERVIDOR
    // ============================================================

    println!("[6/6] Iniciando servidor...");

    let listener =
        tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await?;

    tracing::info!(
        "theo backend (rust) ouvindo em http://0.0.0.0:{}",
        config.port
    );

    println!();
    println!("========================================");
    println!("       THEO BACKEND ONLINE");
    println!("========================================");
    println!("Servidor: http://0.0.0.0:{}", config.port);
    println!();
    println!("Endpoints principais:");
    println!("  POST /api/auth/...");
    println!("  GET  /api/dashboard/...");
    println!("  GET  /api/stats/...");
    println!("  POST /api/reviews/...");
    println!("  POST /api/ai/chat");
    println!("  GET  /api/ai/recommendations");
    println!("========================================");
    println!();

    axum::serve(listener, app).await?;

    Ok(())
}