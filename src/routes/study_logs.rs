use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::exam::{Exam, StudyLog, TopicReview},
    review_ladder,
    state::AppState,
};
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(index).post(create))
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub exam_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
}

async fn index(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<LogsQuery>,
) -> AppResult<Json<Vec<StudyLog>>> {
    let logs: Vec<StudyLog> = match (q.exam_id, q.topic_id) {
        (_, Some(topic_id)) => {
            sqlx::query_as("select * from study_logs where user_id = $1 and topic_id = $2 order by studied_on desc")
                .bind(user.id)
                .bind(topic_id)
                .fetch_all(&state.db)
                .await?
        }
        (Some(exam_id), None) => {
            sqlx::query_as("select * from study_logs where user_id = $1 and exam_id = $2 order by studied_on desc")
                .bind(user.id)
                .bind(exam_id)
                .fetch_all(&state.db)
                .await?
        }
        (None, None) => {
            sqlx::query_as("select * from study_logs where user_id = $1 order by studied_on desc limit 200")
                .bind(user.id)
                .fetch_all(&state.db)
                .await?
        }
    };

    Ok(Json(logs))
}

#[derive(Debug, Deserialize)]
pub struct StudyLogPayload {
    pub exam_id: Uuid,
    pub topic_id: Uuid,
    #[serde(default = "default_category")]
    pub category: String, // teoria | revisao | questoes | videoaula | leitura
    pub studied_on: NaiveDate,
    #[serde(default)]
    pub minutes: i32,
    pub pages: Option<i32>,
    pub video_minutes: Option<i32>,
    pub questions_total: Option<i32>,
    pub questions_correct: Option<i32>,
    pub notes: Option<String>,
    /// Se true, entra (ou avança) na escada de revisão espaçada deste tópico.
    #[serde(default)]
    pub schedule_reviews: bool,
}

fn default_category() -> String {
    "teoria".to_string()
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<StudyLogPayload>,
) -> AppResult<Json<serde_json::Value>> {
    Ok(Json(create_log_internal(&state, user.id, payload).await?))
}

/// Núcleo da criação de registro de estudo (validação + escada de revisão), reaproveitado
/// tanto pelo endpoint HTTP `POST /study-logs` quanto por `POST /timer/finish`.
pub async fn create_log_internal(
    state: &AppState,
    user_id: Uuid,
    payload: StudyLogPayload,
) -> AppResult<serde_json::Value> {
    let today = Utc::now().date_naive();
    if payload.studied_on > today {
        return Err(AppError::Validation("não é possível registrar estudo em data futura".into()));
    }

    let exam: Exam = sqlx::query_as("select * from exams where id = $1 and user_id = $2")
        .bind(payload.exam_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let topic_owned: i64 = sqlx::query_scalar("select count(*) from exam_topics where id = $1 and exam_id = $2")
        .bind(payload.topic_id)
        .bind(payload.exam_id)
        .fetch_one(&state.db)
        .await?;
    if topic_owned == 0 {
        return Err(AppError::NotFound);
    }

    let logged_same_day = payload.studied_on == today;

    let log: StudyLog = sqlx::query_as(
        r#"insert into study_logs
           (user_id, exam_id, topic_id, category, studied_on, minutes, pages, video_minutes,
            questions_total, questions_correct, notes, logged_same_day)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
           returning *"#,
    )
    .bind(user_id)
    .bind(payload.exam_id)
    .bind(payload.topic_id)
    .bind(&payload.category)
    .bind(payload.studied_on)
    .bind(payload.minutes)
    .bind(payload.pages)
    .bind(payload.video_minutes)
    .bind(payload.questions_total)
    .bind(payload.questions_correct)
    .bind(&payload.notes)
    .bind(logged_same_day)
    .fetch_one(&state.db)
    .await?;

    let mut review_action = "none";

    if payload.category == "revisao" {
        // Se há revisão pendente mais próxima, este registro a conclui em vez de ficar solta.
        let pending: Option<TopicReview> = sqlx::query_as(
            r#"select * from topic_reviews
               where topic_id = $1 and user_id = $2 and completed_at is null and skipped = false
               order by scheduled_date asc limit 1"#,
        )
        .bind(payload.topic_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;

        if let Some(review) = pending {
            review_ladder::complete_and_advance(state, &exam, review.id, payload.topic_id, user_id, log.id, review.step).await?;
            review_action = "completed_pending";
        } else if payload.schedule_reviews {
            review_ladder::start_ladder(state, &exam, payload.topic_id, user_id, log.id, payload.studied_on).await?;
            review_action = "started_ladder";
        }
    } else if payload.schedule_reviews {
        let already_pending: i64 = sqlx::query_scalar(
            "select count(*) from topic_reviews where topic_id = $1 and user_id = $2 and completed_at is null and skipped = false",
        )
        .bind(payload.topic_id)
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;

        if already_pending == 0 {
            review_ladder::start_ladder(state, &exam, payload.topic_id, user_id, log.id, payload.studied_on).await?;
            review_action = "started_ladder";
        }
    }

    Ok(json!({ "log": log, "review_action": review_action }))
}
