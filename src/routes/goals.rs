use crate::{auth::middleware::CurrentUser, error::{AppError, AppResult}, models::goal::Goal, state::AppState};
use axum::{extract::{Path, State}, routing::get, Json, Router};
use chrono::{Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index).post(create))
        .route("/:id", axum::routing::put(update).delete(destroy))
}

async fn index(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<GoalWithProgress>>> {
    let goals: Vec<Goal> = sqlx::query_as("select * from goals where user_id = $1 and active = true order by inserted_at desc")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;

    let mut out = Vec::with_capacity(goals.len());
    for goal in goals {
        let progress = compute_progress(&state, &goal).await?;
        out.push(GoalWithProgress { goal, progress });
    }
    Ok(Json(out))
}

#[derive(Debug, Serialize)]
struct GoalWithProgress {
    #[serde(flatten)]
    goal: Goal,
    progress: f64,
}

/// Calcula quanto já foi realizado dentro do período da meta (dia/semana/mês atual),
/// somando os registros de estudo do usuário (de um edital específico, se a meta for
/// presa a um, ou de todos, se for uma meta geral).
async fn compute_progress(state: &AppState, goal: &Goal) -> AppResult<f64> {
    let today = Utc::now().date_naive();
    let period_start = match goal.period.as_str() {
        "daily" => today,
        "weekly" => today - Duration::days(today.weekday().num_days_from_sunday() as i64),
        "monthly" => today.with_day(1).unwrap_or(today),
        _ => today,
    };

    let value: Option<f64> = match goal.metric.as_str() {
        "minutes" => {
            sqlx::query_scalar(
                "select coalesce(sum(minutes), 0)::float8 from study_logs
                 where user_id = $1 and studied_on >= $2 and ($3::uuid is null or exam_id = $3)",
            )
            .bind(goal.user_id)
            .bind(period_start)
            .bind(goal.exam_id)
            .fetch_one(&state.db)
            .await?
        }
        "questions" => {
            sqlx::query_scalar(
                "select coalesce(sum(questions_total), 0)::float8 from study_logs
                 where user_id = $1 and studied_on >= $2 and ($3::uuid is null or exam_id = $3)",
            )
            .bind(goal.user_id)
            .bind(period_start)
            .bind(goal.exam_id)
            .fetch_one(&state.db)
            .await?
        }
        "reviews" => {
            sqlx::query_scalar(
                "select count(*)::float8 from topic_reviews r join exam_topics t on t.id = r.topic_id
                 where r.user_id = $1 and r.completed_at >= $2 and ($3::uuid is null or t.exam_id = $3)",
            )
            .bind(goal.user_id)
            .bind(period_start.and_hms_opt(0, 0, 0).unwrap().and_utc())
            .bind(goal.exam_id)
            .fetch_one(&state.db)
            .await?
        }
        "topics" => {
            sqlx::query_scalar(
                "select count(distinct topic_id)::float8 from study_logs
                 where user_id = $1 and studied_on >= $2 and ($3::uuid is null or exam_id = $3)",
            )
            .bind(goal.user_id)
            .bind(period_start)
            .bind(goal.exam_id)
            .fetch_one(&state.db)
            .await?
        }
        _ => None,
    };

    Ok(value.unwrap_or(0.0))
}

#[derive(Debug, Deserialize)]
pub struct GoalPayload {
    pub period: String,
    pub metric: String,
    pub target: f64,
    pub exam_id: Option<Uuid>,
}

fn validate(payload: &GoalPayload) -> AppResult<()> {
    if !["daily", "weekly", "monthly"].contains(&payload.period.as_str()) {
        return Err(AppError::Validation("period deve ser daily, weekly ou monthly".into()));
    }
    if !["minutes", "questions", "reviews", "topics"].contains(&payload.metric.as_str()) {
        return Err(AppError::Validation("metric deve ser minutes, questions, reviews ou topics".into()));
    }
    if payload.target <= 0.0 {
        return Err(AppError::Validation("target deve ser maior que zero".into()));
    }
    Ok(())
}

async fn create(State(state): State<AppState>, user: CurrentUser, Json(payload): Json<GoalPayload>) -> AppResult<Json<Goal>> {
    validate(&payload)?;
    let goal = sqlx::query_as::<_, Goal>(
        "insert into goals (user_id, period, metric, target, exam_id) values ($1, $2, $3, $4, $5) returning *",
    )
    .bind(user.id)
    .bind(&payload.period)
    .bind(&payload.metric)
    .bind(payload.target)
    .bind(payload.exam_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(goal))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<GoalPayload>,
) -> AppResult<Json<Goal>> {
    validate(&payload)?;
    let goal = sqlx::query_as::<_, Goal>(
        "update goals set period = $1, metric = $2, target = $3, exam_id = $4, updated_at = now()
         where id = $5 and user_id = $6 returning *",
    )
    .bind(&payload.period)
    .bind(&payload.metric)
    .bind(payload.target)
    .bind(payload.exam_id)
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(goal))
}

async fn destroy(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    let result = sqlx::query("update goals set active = false where id = $1 and user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "message": "meta removida" })))
}
