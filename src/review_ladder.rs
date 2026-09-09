// A "escada" de revisão espaçada: nasce do registro de estudo, nunca é criada à mão.
// Cada revisão concluída gera a próxima; ao passar do último degrau, recomeça do primeiro.

use crate::{error::AppResult, models::exam::Exam, state::AppState};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use uuid::Uuid;

const DEFAULT_LADDER_DAYS: [i32; 4] = [1, 7, 30, 90];

fn ladder(exam: &Exam) -> Vec<i32> {
    let days = exam.review_intervals();
    if days.is_empty() { DEFAULT_LADDER_DAYS.to_vec() } else { days }
}

/// Calcula a data ideal do próximo degrau e aplica as regras de bloqueio:
/// - modo "livre": pula dias excluídos pelo usuário, avançando dia a dia
/// - modo "com prazo certo": nunca agenda dentro do sprint final nem depois do dia seguinte à prova
/// Retorna `None` quando a revisão não deve ser criada (cai numa janela bloqueada sem alternativa razoável).
fn next_scheduled_date(exam: &Exam, from: NaiveDate, interval_days: i32) -> Option<NaiveDate> {
    let mut candidate = from + Duration::days(interval_days as i64);

    if exam.plan_mode == "deadline" {
        if let (Some(exam_date), Some(sprint_days)) = (exam.exam_date, exam.sprint_days) {
            let sprint_start = exam_date - Duration::days(sprint_days as i64);
            let last_valid = exam_date + Duration::days(1);

            if candidate >= sprint_start || candidate > last_valid {
                return None;
            }
        }
    }

    // Dias da semana excluídos pelo usuário (principalmente no modo livre): avança até achar um dia livre,
    // mas sem nunca invadir o sprint/pós-prova checado acima.
    for _ in 0..14 {
        let weekday = candidate.weekday().num_days_from_sunday() as i32;
        if !exam.excluded_weekdays_vec().contains(&weekday) {
            return Some(candidate);
        }
        candidate += Duration::days(1);

        if exam.plan_mode == "deadline" {
            if let (Some(exam_date), Some(sprint_days)) = (exam.exam_date, exam.sprint_days) {
                let sprint_start = exam_date - Duration::days(sprint_days as i64);
                if candidate >= sprint_start {
                    return None;
                }
            }
        }
    }

    None
}

/// Cria o primeiro degrau da escada para um tópico, ancorado na data do estudo.
pub async fn start_ladder(
    state: &AppState,
    exam: &Exam,
    topic_id: Uuid,
    user_id: Uuid,
    study_log_id: Uuid,
    studied_on: NaiveDate,
) -> AppResult<()> {
    let steps = ladder(exam);
    if let Some(first_interval) = steps.first() {
        if let Some(date) = next_scheduled_date(exam, studied_on, *first_interval) {
            sqlx::query(
                "insert into topic_reviews (user_id, topic_id, study_log_id, step, scheduled_date)
                 values ($1, $2, $3, 1, $4)",
            )
            .bind(user_id)
            .bind(topic_id)
            .bind(study_log_id)
            .bind(date)
            .execute(&state.db)
            .await?;
        }
    }
    Ok(())
}

/// Marca uma revisão como concluída e cria o próximo degrau (recomeçando do 1 se acabou a escada).
pub async fn complete_and_advance(
    state: &AppState,
    exam: &Exam,
    review_id: Uuid,
    topic_id: Uuid,
    user_id: Uuid,
    completed_study_log_id: Uuid,
    current_step: i32,
) -> AppResult<()> {
    sqlx::query(
        "update topic_reviews set completed_at = CURRENT_TIMESTAMP, completed_study_log_id = $1 where id = $2",
    )
    .bind(completed_study_log_id)
    .bind(review_id)
    .execute(&state.db)
    .await?;

    let steps = ladder(exam);
    let next_step_number = if (current_step as usize) >= steps.len() { 1 } else { current_step + 1 };
    let interval = steps
        .get((next_step_number - 1) as usize)
        .copied()
        .unwrap_or(DEFAULT_LADDER_DAYS[0]);

    let today = Utc::now().date_naive();
    if let Some(date) = next_scheduled_date(exam, today, interval) {
        sqlx::query(
            "insert into topic_reviews (user_id, topic_id, study_log_id, step, scheduled_date)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(user_id)
        .bind(topic_id)
        .bind(completed_study_log_id)
        .bind(next_step_number)
        .bind(date)
        .execute(&state.db)
        .await?;
    }

    Ok(())
}
