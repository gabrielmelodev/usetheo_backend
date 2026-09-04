use crate::{auth::middleware::CurrentUser, error::AppResult, state::AppState};
use axum::{extract::State, routing::get, Json, Router};
use chrono::{Duration, Utc};
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/summary", get(summary))
}

/// Resumo diário global da Home — agregando TODOS os editais/planos do usuário, não um só.
/// Cobre o que a Home precisa mostrar de cara: horas de hoje, meta do dia, questões,
/// revisões pendentes, sequência e o diagnóstico automático (matéria mais forte/mais fraca).
async fn summary(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    let today = Utc::now().date_naive();

    let minutes_today: i64 = sqlx::query_scalar(
        "select coalesce(sum(minutes), 0) from study_logs where user_id = $1 and studied_on = $2",
    )
    .bind(user.id)
    .bind(today)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let (questions_total_today, questions_correct_today): (i64, i64) = sqlx::query_as(
        r#"select coalesce(sum(questions_total), 0), coalesce(sum(questions_correct), 0)
           from study_logs where user_id = $1 and studied_on = $2"#,
    )
    .bind(user.id)
    .bind(today)
    .fetch_one(&state.db)
    .await
    .unwrap_or((0, 0));

    let reviews_pending_today: i64 = sqlx::query_scalar(
        "select count(*) from topic_reviews where user_id = $1 and scheduled_date <= $2 and completed_at is null and skipped = false",
    )
    .bind(user.id)
    .bind(today)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let topics_studied_today: i64 = sqlx::query_scalar(
        "select count(distinct topic_id) from study_logs where user_id = $1 and studied_on = $2",
    )
    .bind(user.id)
    .bind(today)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // Sequência: dias consecutivos com registro lançado no próprio dia, entre TODOS os editais.
    let studied_dates: Vec<chrono::NaiveDate> = sqlx::query_scalar(
        "select distinct studied_on from study_logs where user_id = $1 and logged_same_day = true order by studied_on desc",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

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

    // Diagnóstico: matéria mais forte/mais fraca por % de acerto em questões, entre todos os editais.
    let performance: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"select s.name, coalesce(sum(l.questions_total), 0), coalesce(sum(l.questions_correct), 0)
           from exam_subjects s
           join exam_topics t on t.subject_id = s.id
           join study_logs l on l.topic_id = t.id
           where s.exam_id in (select id from exams where user_id = $1)
           group by s.id, s.name
           having coalesce(sum(l.questions_total), 0) > 0"#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let mut ranked: Vec<(String, f64)> = performance
        .into_iter()
        .map(|(name, total, correct)| (name, if total > 0 { correct as f64 / total as f64 } else { 0.0 }))
        .collect();
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let weakest = ranked.first().cloned();
    let strongest = ranked.last().cloned();

    // Próxima atividade sugerida: a revisão mais atrasada, senão o próximo tópico agendado.
    let next_review: Option<(String, String, chrono::NaiveDate)> = sqlx::query_as(
        r#"select t.number, t.title, r.scheduled_date from topic_reviews r
           join exam_topics t on t.id = r.topic_id
           where r.user_id = $1 and r.completed_at is null and r.skipped = false
           order by r.scheduled_date asc limit 1"#,
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    Ok(Json(json!({
        "today": {
            "minutes_studied": minutes_today,
            "questions_total": questions_total_today,
            "questions_correct": questions_correct_today,
            "topics_studied": topics_studied_today,
        },
        "reviews_pending": reviews_pending_today,
        "streak_days": streak,
        "diagnosis": {
            "weakest_subject": weakest.map(|(n, a)| json!({ "name": n, "accuracy": a })),
            "strongest_subject": strongest.map(|(n, a)| json!({ "name": n, "accuracy": a })),
        },
        "next_review": next_review.map(|(number, title, date)| json!({
            "topic_number": number, "topic_title": title, "scheduled_date": date,
        })),
    })))
}
