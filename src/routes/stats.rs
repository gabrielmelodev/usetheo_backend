use crate::{
    auth::middleware::CurrentUser,
    error::AppResult,
    state::AppState,
};

use axum::{
    extract::State,
    routing::get,
    Json,
    Router,
};

use serde::Serialize;
use chrono::{Duration, Utc};

pub fn router() -> Router<AppState> {
    Router::new().route("/overview", get(overview))
}

#[derive(Debug, Serialize)]
struct Overview {
    total_decks: i64,
    total_cards: i64,
    due_today: i64,
    reviews_last_7_days: i64,
}

async fn overview(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Overview>> {
    let total_decks: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM decks
        WHERE user_id = $1
        "#,
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    let total_cards: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM cards
        WHERE user_id = $1
        "#,
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    let due_today: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM cards
        WHERE user_id = $1
          AND due <= CURRENT_TIMESTAMP
        "#,
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    let reviews_last_7_days: i64 = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM review_logs
        WHERE user_id = $1
          AND reviewed_at >= $2
        "#,
    )
    .bind(user.id)
    .bind(Utc::now() - Duration::days(7))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(Overview {
        total_decks,
        total_cards,
        due_today,
        reviews_last_7_days,
    }))
}