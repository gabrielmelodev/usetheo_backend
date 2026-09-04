use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/due", get(due))
        .route("/:id/skip", post(skip))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct DueReview {
    id: Uuid,
    topic_id: Uuid,
    exam_id: Uuid,
    topic_number: String,
    topic_title: String,
    step: i32,
    scheduled_date: chrono::NaiveDate,
    days_overdue: i32,
}

#[derive(Debug, Deserialize)]
pub struct DueQuery {
    pub exam_id: Option<Uuid>,
}

/// Revisões pendentes, incluindo atrasadas (`days_overdue > 0`). Base para os marcadores
/// "Xd DD/MM" (agendada) e "Xd DD/MM" em vermelho (atrasada) no edital esquematizado.
async fn due(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<DueQuery>,
) -> AppResult<Json<Vec<DueReview>>> {
    let today = Utc::now().date_naive();

    let rows: Vec<DueReview> = sqlx::query_as(
        r#"select r.id, r.topic_id, t.exam_id, t.number as topic_number, t.title as topic_title,
                  r.step, r.scheduled_date,
                  greatest(0, ($1::date - r.scheduled_date))::int as days_overdue
           from topic_reviews r
           join exam_topics t on t.id = r.topic_id
           where r.user_id = $2 and r.completed_at is null and r.skipped = false
             and ($3::uuid is null or t.exam_id = $3)
           order by r.scheduled_date asc"#,
    )
    .bind(today)
    .bind(user.id)
    .bind(q.exam_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

async fn skip(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    let owned: i64 = sqlx::query_scalar("select count(*) from topic_reviews where id = $1 and user_id = $2")
        .bind(id)
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if owned == 0 {
        return Err(AppError::NotFound);
    }

    sqlx::query("update topic_reviews set skipped = true where id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "message": "revisão ignorada" })))
}
