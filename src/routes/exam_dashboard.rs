use crate::{auth::middleware::CurrentUser, error::{AppError, AppResult}, state::AppState};
use axum::{extract::{Path, State}, routing::get, Json, Router};
use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/:id/topics-with-markers", get(topics_with_markers))
        .route("/:id/stats", get(stats))
}

async fn owned(state: &AppState, user_id: Uuid, exam_id: Uuid) -> AppResult<()> {
    let n: i64 = sqlx::query_scalar("select count(*) from exams where id = $1 and user_id = $2")
        .bind(exam_id)
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct TopicMarker {
    id: Uuid,
    subject_id: Uuid,
    number: String,
    title: String,
    relevance: i32,
    in_sprint: bool,
    scheduled_date: Option<chrono::NaiveDate>,

    // Deriva tudo isso a partir de study_logs / topic_reviews — nenhum é guardado à parte.
    status: String, // not_started | studied
    first_studied_on: Option<chrono::NaiveDate>,
    last_log_on: Option<chrono::NaiveDate>,
    log_count: i64,
    next_review_date: Option<chrono::NaiveDate>,
    next_review_step: Option<i32>,
    review_overdue_days: Option<i32>,
}

/// Cada tópico do edital com os marcadores prontos para o "Edital esquematizado":
/// não estudado / planejado / estudei / revisei / revisão pendente (com atraso, se houver).
async fn topics_with_markers(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Vec<TopicMarker>>> {
    owned(&state, user.id, id).await?;
    let today = Utc::now().date_naive();

    let rows: Vec<TopicMarker> = sqlx::query_as(
        r#"select
             t.id, t.subject_id, t.number, t.title, t.relevance, t.in_sprint, t.scheduled_date,
             case when (select count(*) from study_logs where topic_id = t.id) > 0 then 'studied' else 'not_started' end as status,
             (select min(studied_on) from study_logs where topic_id = t.id) as first_studied_on,
             (select max(studied_on) from study_logs where topic_id = t.id) as last_log_on,
             (select count(*) from study_logs where topic_id = t.id) as log_count,
             (select scheduled_date from topic_reviews where topic_id = t.id and completed_at is null and skipped = false order by scheduled_date asc limit 1) as next_review_date,
             (select step from topic_reviews where topic_id = t.id and completed_at is null and skipped = false order by scheduled_date asc limit 1) as next_review_step,
             case when (select scheduled_date from topic_reviews where topic_id = t.id and completed_at is null and skipped = false order by scheduled_date asc limit 1) is not null
                  and (select scheduled_date from topic_reviews where topic_id = t.id and completed_at is null and skipped = false order by scheduled_date asc limit 1) < $2
                  then CAST(julianday($2) - julianday((select scheduled_date from topic_reviews where topic_id = t.id and completed_at is null and skipped = false order by scheduled_date asc limit 1)) AS INTEGER) else null end as review_overdue_days
           from exam_topics t
           where t.exam_id = $1
           order by t.position"#,
    )
    .bind(id)
    .bind(today)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

#[derive(Debug, Serialize)]
struct SubjectPerformance {
    subject_id: Uuid,
    subject_name: String,
    questions_total: i64,
    questions_correct: i64,
    accuracy: f64,
}

/// Progresso do edital, constância (sequência de dias com registro feito no próprio dia)
/// e o "raio-x": desempenho por matéria, do pior para o melhor acerto.
async fn stats(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    owned(&state, user.id, id).await?;

    let total_topics: i64 = sqlx::query_scalar("select count(*) from exam_topics where exam_id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    let studied_topics: i64 = sqlx::query_scalar(
        r#"select count(distinct t.id) from exam_topics t
           join study_logs l on l.topic_id = t.id
           where t.exam_id = $1"#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    // Constância: só conta dias em que o registro foi lançado no próprio dia (logged_same_day).
    let studied_dates: Vec<chrono::NaiveDate> = sqlx::query_scalar(
        r#"select distinct studied_on from study_logs
           where exam_id = $1 and user_id = $2 and logged_same_day = true
           order by studied_on desc"#,
    )
    .bind(id)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let today = Utc::now().date_naive();
    let mut streak = 0i32;
    let mut cursor = today;
    for date in &studied_dates {
        if *date == cursor {
            streak += 1;
            cursor -= Duration::days(1);
        } else if *date < cursor {
            break;
        }
    }

    let performance: Vec<SubjectPerformance> = sqlx::query_as::<_, (Uuid, String, Option<i64>, Option<i64>)>(
        r#"select s.id, s.name, sum(l.questions_total), sum(l.questions_correct)
           from exam_subjects s
           join exam_topics t on t.subject_id = s.id
           left join study_logs l on l.topic_id = t.id
           where s.exam_id = $1
           group by s.id, s.name"#,
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(subject_id, subject_name, total, correct)| {
        let total = total.unwrap_or(0);
        let correct = correct.unwrap_or(0);
        let accuracy = if total > 0 { correct as f64 / total as f64 } else { 0.0 };
        SubjectPerformance { subject_id, subject_name, questions_total: total, questions_correct: correct, accuracy }
    })
    .collect();

    let mut performance = performance;
    performance.sort_by(|a, b| a.accuracy.partial_cmp(&b.accuracy).unwrap());

    Ok(Json(json!({
        "progress": { "total_topics": total_topics, "studied_topics": studied_topics },
        "streak_days": streak,
        "performance": performance,
    })))
}
