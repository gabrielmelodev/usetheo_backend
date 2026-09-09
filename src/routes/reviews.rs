use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::card::Card,
    routes::cards::render_cards,
    srs::{self, Rating},
    state::AppState,
};
use axum::{extract::State, routing::{get, post}, Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/queue", get(queue))
        .route("/submit", post(submit))
}

#[derive(Debug, Deserialize)]
pub struct QueueQuery {
    pub deck_id: Option<Uuid>,
    pub limit: Option<i64>,
}

async fn queue(
    State(state): State<AppState>,
    user: CurrentUser,
    axum::extract::Query(q): axum::extract::Query<QueueQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let cards: Vec<Card> = if let Some(deck_id) = q.deck_id {
        let owned: i64 = sqlx::query_scalar("select count(*) from decks where id = $1 and user_id = $2")
            .bind(deck_id)
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
        if owned == 0 {
            return Err(AppError::NotFound);
        }

        sqlx::query_as(
            "select * from cards where user_id = $1 and deck_id = $2 and due <= CURRENT_TIMESTAMP order by due limit $3",
        )
        .bind(user.id)
        .bind(deck_id)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as("select * from cards where user_id = $1 and due <= CURRENT_TIMESTAMP order by due limit $2")
            .bind(user.id)
            .bind(limit)
            .fetch_all(&state.db)
            .await?
    };

    let rendered = render_cards(&state, cards).await?;
    Ok(Json(json!({ "cards": rendered, "count": rendered.len() })))
}

#[derive(Debug, Deserialize)]
pub struct SubmitPayload {
    pub card_id: Uuid,
    pub rating: String,
}

async fn submit(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<SubmitPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let rating = Rating::from_str(&payload.rating)
        .ok_or_else(|| AppError::Validation("rating deve ser again, hard, good ou easy".to_string()))?;

    let card: Card = sqlx::query_as("select * from cards where id = $1 and user_id = $2")
        .bind(payload.card_id)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let result = srs::apply_review(rating, card.interval_days, card.ease_factor, card.repetitions, card.lapses);

    let due = chrono::Utc::now() + chrono::Duration::seconds((result.interval_days * 86400.0) as i64);

    sqlx::query(
        r#"update cards set queue = $1, due = $2, interval_days = $3, ease_factor = $4,
           repetitions = $5, lapses = $6, updated_at = CURRENT_TIMESTAMP where id = $7"#,
    )
    .bind(&result.queue)
    .bind(due)
    .bind(result.interval_days)
    .bind(result.ease_factor)
    .bind(result.repetitions)
    .bind(result.lapses)
    .bind(card.id)
    .execute(&state.db)
    .await?;

    sqlx::query(
        r#"insert into review_logs (card_id, user_id, rating, interval_before, interval_after, ease_before, ease_after)
           values ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(card.id)
    .bind(user.id)
    .bind(&payload.rating)
    .bind(card.interval_days)
    .bind(result.interval_days)
    .bind(card.ease_factor)
    .bind(result.ease_factor)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "card_id": card.id,
        "queue": result.queue,
        "due": due,
        "interval_days": result.interval_days,
        "ease_factor": result.ease_factor,
    })))
}
