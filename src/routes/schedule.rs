use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::exam::{Exam, ExamSubject, ExamTopic, ManualScheduleEntry},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/:id/generate-schedule", post(generate_deadline_schedule))
        .route("/:id/schedule", get(continuous_schedule))
        .route("/:id/manual-schedule", get(list_manual).post(create_manual))
        .route("/:id/schedule-check", get(schedule_check))
}

async fn find_owned(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Exam> {
    sqlx::query_as::<_, Exam>("select * from exams where id = $1 and user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

// ---------------- Modo "Com prazo certo" ----------------

/// Distribui os tópicos fora do sprint pelos dias entre hoje e o início do sprint, e os
/// tópicos do sprint (por relevância) pelos dias do sprint. Não há meta de horas: só datas.
async fn generate_deadline_schedule(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let exam = find_owned(&state, user.id, id).await?;
    if exam.plan_mode != "deadline" {
        return Err(AppError::Validation("este edital não está no modo 'com prazo certo'".into()));
    }
    let exam_date = exam.exam_date.ok_or_else(|| AppError::Validation("edital sem exam_date".into()))?;
    let sprint_days = exam.sprint_days.unwrap_or(7);
    let sprint_start = exam_date - Duration::days(sprint_days as i64);
    let today = Utc::now().date_naive();

    if sprint_start <= today {
        return Err(AppError::Validation("não sobra tempo antes do sprint final com a data atual".into()));
    }

    let available_days = |start: NaiveDate, end: NaiveDate| -> Vec<NaiveDate> {
        let mut days = Vec::new();
        let mut d = start;
        while d < end {
            let weekday = d.weekday().num_days_from_sunday() as i32;
            if !exam.excluded_weekdays.contains(&weekday) {
                days.push(d);
            }
            d += Duration::days(1);
        }
        days
    };

    let regular_days = available_days(today, sprint_start);
    let sprint_window_days = available_days(sprint_start, exam_date);

    if regular_days.is_empty() {
        return Err(AppError::Validation("nenhum dia disponível antes do sprint (confira os dias excluídos)".into()));
    }

    let mut topics: Vec<ExamTopic> = sqlx::query_as("select * from exam_topics where exam_id = $1 order by position")
        .bind(id)
        .fetch_all(&state.db)
        .await?;

    let (mut sprint_topics, mut regular_topics): (Vec<ExamTopic>, Vec<ExamTopic>) =
        (Vec::new(), Vec::new());
    for t in topics.drain(..) {
        if t.in_sprint {
            sprint_topics.push(t);
        } else {
            regular_topics.push(t);
        }
    }
    // Sprint: mais relevante primeiro, para garantir que o que mais cai é revisado com folga.
    sprint_topics.sort_by(|a, b| b.relevance.cmp(&a.relevance));

    let mut tx = state.db.begin().await?;

    for (i, topic) in regular_topics.iter().enumerate() {
        let date = regular_days[i % regular_days.len()];
        sqlx::query("update exam_topics set scheduled_date = $1 where id = $2")
            .bind(date)
            .bind(topic.id)
            .execute(&mut *tx)
            .await?;
    }

    if !sprint_topics.is_empty() {
        if sprint_window_days.is_empty() {
            return Err(AppError::Validation("nenhum dia disponível dentro do sprint (confira os dias excluídos)".into()));
        }
        for (i, topic) in sprint_topics.iter().enumerate() {
            let date = sprint_window_days[i % sprint_window_days.len()];
            sqlx::query("update exam_topics set scheduled_date = $1 where id = $2")
                .bind(date)
                .bind(topic.id)
                .execute(&mut *tx)
                .await?;
        }
    }

    tx.commit().await?;

    Ok(Json(json!({
        "message": "cronograma gerado",
        "regular_topics": regular_topics.len(),
        "sprint_topics": sprint_topics.len(),
        "sprint_start": sprint_start,
    })))
}

// ---------------- Modo "Contínuo" ----------------

#[derive(Debug, Serialize)]
struct SubjectAllocation {
    subject_id: Uuid,
    subject_name: String,
    effective_weight: f64,
    weekly_minutes: i32,
}

#[derive(Debug, Deserialize)]
pub struct ContinuousQuery {
    pub mode: Option<String>, // semanal | ciclo (default: o salvo no exame)
}

/// Calcula (sem persistir) a divisão de horas por matéria — "peso × tópicos restantes ×
/// conhecimento" — e monta a visão semanal ou o ciclo, conforme pedido.
async fn continuous_schedule(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<ContinuousQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let exam = find_owned(&state, user.id, id).await?;
    if exam.plan_mode != "continuous" {
        return Err(AppError::Validation("este edital não está no modo 'contínuo'".into()));
    }
    let weekly_hours = exam.weekly_hours.unwrap_or(0.0);
    let mode = q.mode.or_else(|| exam.continuous_mode.clone()).unwrap_or_else(|| "semanal".to_string());

    let subjects: Vec<ExamSubject> = sqlx::query_as("select * from exam_subjects where exam_id = $1 order by position")
        .bind(id)
        .fetch_all(&state.db)
        .await?;

    let mut remaining_by_subject: HashMap<Uuid, i64> = HashMap::new();
    for s in &subjects {
        // "tópicos restantes" = tópicos sem nenhum registro de estudo ainda.
        let remaining: i64 = sqlx::query_scalar(
            r#"select count(*) from exam_topics t
               where t.subject_id = $1
                 and not exists (select 1 from study_logs l where l.topic_id = t.id)"#,
        )
        .bind(s.id)
        .fetch_one(&state.db)
        .await?;
        remaining_by_subject.insert(s.id, remaining);
    }

    let allocations: Vec<SubjectAllocation> = {
        let weighted: Vec<(f64, &ExamSubject)> = subjects
            .iter()
            .filter(|s| remaining_by_subject.get(&s.id).copied().unwrap_or(0) > 0)
            .map(|s| {
                let remaining = remaining_by_subject.get(&s.id).copied().unwrap_or(0) as f64;
                let effective = s.weight as f64 * remaining * s.knowledge_factor;
                (effective, s)
            })
            .collect();

        let total: f64 = weighted.iter().map(|(w, _)| w).sum();

        weighted
            .into_iter()
            .map(|(effective, s)| {
                let share = if total > 0.0 { effective / total } else { 0.0 };
                SubjectAllocation {
                    subject_id: s.id,
                    subject_name: s.name.clone(),
                    effective_weight: effective,
                    weekly_minutes: (weekly_hours * 60.0 * share).round() as i32,
                }
            })
            .collect()
    };

    if mode == "ciclo" {
        Ok(Json(json!({
            "mode": "ciclo",
            "queue": build_cycle_queue(&allocations, exam.max_session_minutes.unwrap_or(60)),
        })))
    } else {
        Ok(Json(json!({
            "mode": "semanal",
            "week": build_weekly_grid(&allocations, &exam.excluded_weekdays),
        })))
    }
}

/// Sessões de no máximo `max_session_minutes`, intercalando matérias para que a mesma
/// nunca apareça duas vezes seguidas na fila.
fn build_cycle_queue(
    allocations: &[SubjectAllocation],
    max_session_minutes: i32,
) -> Vec<serde_json::Value> {
    let max_session_minutes = max_session_minutes.max(15);

    let mut per_subject_sessions: Vec<(Uuid, String, Vec<i32>)> = allocations
        .iter()
        .map(|a| {
            let mut sessions = Vec::new();
            let mut remaining = a.weekly_minutes;

            while remaining > 0 {
                let chunk = remaining.min(max_session_minutes);
                sessions.push(chunk);
                remaining -= chunk;
            }

            (a.subject_id, a.subject_name.clone(), sessions)
        })
        .filter(|(_, _, sessions)| !sessions.is_empty())
        .collect();

    let mut queue = Vec::new();
    let mut last_subject: Option<Uuid> = None;

    // Round-robin:
    // a cada volta, tenta pegar uma sessão de cada matéria.
    // Se a matéria atual for igual à última e existir outra matéria
    // com sessões disponíveis, pula para evitar repetição consecutiva.
    loop {
        let mut progressed = false;

        for i in 0..per_subject_sessions.len() {
            if per_subject_sessions[i].2.is_empty() {
                continue;
            }

            let subject_id = per_subject_sessions[i].0;

            // Verifica se existe OUTRA matéria com sessões disponíveis.
            let has_alternative = per_subject_sessions
                .iter()
                .enumerate()
                .any(|(j, (_, _, sessions))| j != i && !sessions.is_empty());

            if last_subject == Some(subject_id) && has_alternative {
                continue;
            }

            let name = per_subject_sessions[i].1.clone();
            let minutes = per_subject_sessions[i].2.remove(0);

            queue.push(json!({
                "subject_id": subject_id,
                "subject_name": name,
                "minutes": minutes
            }));

            last_subject = Some(subject_id);
            progressed = true;
        }

        if !progressed {
            break;
        }
    }

    queue
}




/// Reparte os dias da semana (não excluídos) entre as matérias, proporcional às horas de cada uma.
fn build_weekly_grid(allocations: &[SubjectAllocation], excluded_weekdays: &[i32]) -> serde_json::Value {
    let weekday_names = ["Domingo", "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado"];
    let available: Vec<i32> = (0..7).filter(|d| !excluded_weekdays.contains(d)).collect();

    if available.is_empty() || allocations.is_empty() {
        return json!(weekday_names
            .iter()
            .map(|name| json!({ "weekday": name, "subjects": [] }))
            .collect::<Vec<_>>());
    }

    let total_minutes: i32 = allocations.iter().map(|a| a.weekly_minutes).sum();
    let mut day_assignments: Vec<Vec<&SubjectAllocation>> = vec![Vec::new(); available.len()];

    // Método dos maiores restos: cada matéria recebe `dias × (sua fatia de minutos)`.
    let mut fractional: Vec<(usize, f64, i32)> = Vec::new();
    for (i, a) in allocations.iter().enumerate() {
        let share = if total_minutes > 0 { a.weekly_minutes as f64 / total_minutes as f64 } else { 0.0 };
        let raw_days = share * available.len() as f64;
        fractional.push((i, raw_days.fract(), raw_days.floor() as i32));
    }
    let assigned_so_far: i32 = fractional.iter().map(|(_, _, d)| d).sum();
    let mut remaining_slots = available.len() as i32 - assigned_so_far;
    fractional.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut day_count = vec![0i32; allocations.len()];
    for (i, _, base) in &fractional {
        day_count[*i] = *base;
    }
    for (i, _, _) in fractional.iter() {
        if remaining_slots <= 0 {
            break;
        }
        day_count[*i] += 1;
        remaining_slots -= 1;
    }

    let mut day_cursor = 0usize;
    for (i, a) in allocations.iter().enumerate() {
        for _ in 0..day_count[i] {
            if day_cursor >= day_assignments.len() {
                break;
            }
            day_assignments[day_cursor].push(a);
            day_cursor += 1;
        }
    }

    let mut out = Vec::new();
    for (idx, weekday) in available.iter().enumerate() {
        let subjects: Vec<_> = day_assignments[idx]
            .iter()
            .map(|a| json!({ "subject_id": a.subject_id, "subject_name": a.subject_name, "minutes": a.weekly_minutes / day_count[allocations.iter().position(|x| x.subject_id == a.subject_id).unwrap()].max(1) }))
            .collect();
        out.push(json!({ "weekday": weekday_names[*weekday as usize], "subjects": subjects }));
    }
    json!(out)
}

// ---------------- Modo "Livre" ----------------

async fn list_manual(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Vec<ManualScheduleEntry>>> {
    find_owned(&state, user.id, id).await?;
    let entries = sqlx::query_as::<_, ManualScheduleEntry>(
        "select * from manual_schedule_entries where exam_id = $1 order by weekday",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(entries))
}

#[derive(Debug, Deserialize)]
pub struct ManualEntryPayload {
    pub weekday: i32,
    pub subject_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub planned_minutes: Option<i32>,
    pub question_goal: Option<i32>,
}

async fn create_manual(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<ManualEntryPayload>,
) -> AppResult<Json<ManualScheduleEntry>> {
    find_owned(&state, user.id, id).await?;
    if !(0..=6).contains(&payload.weekday) {
        return Err(AppError::Validation("weekday deve ser 0 (domingo) a 6 (sábado)".into()));
    }

    let entry = sqlx::query_as::<_, ManualScheduleEntry>(
        r#"insert into manual_schedule_entries
           (user_id, exam_id, weekday, subject_id, topic_id, planned_minutes, question_goal)
           values ($1, $2, $3, $4, $5, $6, $7) returning *"#,
    )
    .bind(user.id)
    .bind(id)
    .bind(payload.weekday)
    .bind(payload.subject_id)
    .bind(payload.topic_id)
    .bind(payload.planned_minutes)
    .bind(payload.question_goal)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(entry))
}

#[derive(Debug, Serialize)]
struct ScheduleCheckItem {
    weekday: i32,
    planned: bool,
    logged: bool,
}

/// Compara o que foi planejado à mão (modo livre) com o que foi de fato registrado nos
/// últimos 7 dias — a base do aviso "algo que você planejou ficou para trás".
async fn schedule_check(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Vec<ScheduleCheckItem>>> {
    find_owned(&state, user.id, id).await?;
    let today = Utc::now().date_naive();

    let entries: Vec<ManualScheduleEntry> = sqlx::query_as("select * from manual_schedule_entries where exam_id = $1")
        .bind(id)
        .fetch_all(&state.db)
        .await?;

    let mut out = Vec::new();
    for entry in entries {
        // Última ocorrência desse dia da semana (hoje ou anterior).
        let mut date = today;
        while date.weekday().num_days_from_sunday() as i32 != entry.weekday {
            date -= Duration::days(1);
        }

        let logged: i64 = sqlx::query_scalar(
            r#"select count(*) from study_logs
               where exam_id = $1 and user_id = $2 and studied_on = $3
                 and ($4::uuid is null or topic_id = $4)"#,
        )
        .bind(id)
        .bind(user.id)
        .bind(date)
        .bind(entry.topic_id)
        .fetch_one(&state.db)
        .await?;

        out.push(ScheduleCheckItem { weekday: entry.weekday, planned: true, logged: logged > 0 });
    }

    Ok(Json(out))
}
