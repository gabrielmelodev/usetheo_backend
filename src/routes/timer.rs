use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::timer::ActiveTimer,
    state::AppState,
};
use axum::{extract::State, routing::{get, post}, Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/current", get(current))
        .route("/start", post(start))
        .route("/pause", post(pause))
        .route("/resume", post(resume))
        .route("/finish", post(finish))
        .route("/cancel", post(cancel))
}

#[derive(Debug, serde::Serialize)]
struct TimerView {
    #[serde(flatten)]
    timer: ActiveTimer,
    elapsed_seconds: i64,
    subject_name: Option<String>,
    topic_number: Option<String>,
    topic_title: Option<String>,
}

async fn to_view(state: &AppState, timer: ActiveTimer) -> TimerView {
    let elapsed_seconds = timer.elapsed_seconds();

    let names: Option<(String, String, String)> = sqlx::query_as(
        "select s.name, t.number, t.title from exam_topics t
         join exam_subjects s on s.id = t.subject_id
         where t.id = $1",
    )
    .bind(timer.topic_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let (subject_name, topic_number, topic_title) = match names {
        Some((s, n, t)) => (Some(s), Some(n), Some(t)),
        None => (None, None, None),
    };

    TimerView { timer, elapsed_seconds, subject_name, topic_number, topic_title }
}

/// O cronômetro ativo do usuário, se houver — usado pra retomar de outro dispositivo
/// ou depois de fechar o app (o "continuar atividade interrompida" do prompt).
async fn current(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Option<TimerView>>> {
    let timer: Option<ActiveTimer> = sqlx::query_as("select * from active_timers where user_id = $1")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;

    let view = match timer {
        Some(t) => Some(to_view(&state, t).await),
        None => None,
    };

    Ok(Json(view))
}

#[derive(Debug, Deserialize)]
pub struct StartPayload {
    pub exam_id: Uuid,
    pub topic_id: Uuid,
    #[serde(default = "default_category")]
    pub category: String,
}

fn default_category() -> String {
    "teoria".to_string()
}

/// Inicia um cronômetro. Só um por usuário — se já existir um rodando, ele é substituído
/// (o cliente deve avisar o usuário antes de chamar isso se houver um ativo em outra matéria).
async fn start(State(state): State<AppState>, user: CurrentUser, Json(payload): Json<StartPayload>) -> AppResult<Json<TimerView>> {
    let topic_owned: i64 = sqlx::query_scalar("select count(*) from exam_topics where id = $1 and exam_id = $2")
        .bind(payload.topic_id)
        .bind(payload.exam_id)
        .fetch_one(&state.db)
        .await?;
    if topic_owned == 0 {
        return Err(AppError::NotFound);
    }

    sqlx::query("delete from active_timers where user_id = $1").bind(user.id).execute(&state.db).await?;

    let timer: ActiveTimer = sqlx::query_as(
        "insert into active_timers (user_id, exam_id, topic_id, category, running_since)
         values ($1, $2, $3, $4, now()) returning *",
    )
    .bind(user.id)
    .bind(payload.exam_id)
    .bind(payload.topic_id)
    .bind(&payload.category)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(to_view(&state, timer).await))
}

async fn get_current_timer(state: &AppState, user_id: Uuid) -> AppResult<ActiveTimer> {
    sqlx::query_as("select * from active_timers where user_id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn pause(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<TimerView>> {
    let timer = get_current_timer(&state, user.id).await?;
    if timer.running_since.is_none() {
        return Ok(Json(to_view(&state, timer).await)); // já pausado, idempotente
    }

    let elapsed = timer.elapsed_seconds() as i32;
    let updated: ActiveTimer = sqlx::query_as(
        "update active_timers set accumulated_seconds = $1, running_since = null where user_id = $2 returning *",
    )
    .bind(elapsed)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(to_view(&state, updated).await))
}

async fn resume(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<TimerView>> {
    let timer = get_current_timer(&state, user.id).await?;
    if timer.running_since.is_some() {
        return Ok(Json(to_view(&state, timer).await)); // já rodando, idempotente
    }

    let updated: ActiveTimer = sqlx::query_as(
        "update active_timers set running_since = now() where user_id = $1 returning *",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(to_view(&state, updated).await))
}

#[derive(Debug, Deserialize)]
pub struct FinishPayload {
    pub pages: Option<i32>,
    pub video_minutes: Option<i32>,
    pub questions_total: Option<i32>,
    pub questions_correct: Option<i32>,
    pub notes: Option<String>,
    #[serde(default = "default_true")]
    pub schedule_reviews: bool,
}

fn default_true() -> bool {
    true
}

/// Finaliza o cronômetro e gera o registro de estudo automaticamente com o tempo decorrido
/// (reaproveita a mesma lógica de escada de revisão do registro manual).
async fn finish(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<FinishPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let timer = get_current_timer(&state, user.id).await?;
    let elapsed_seconds = timer.elapsed_seconds();
    let minutes = ((elapsed_seconds as f64) / 60.0).round() as i32;

    sqlx::query("delete from active_timers where user_id = $1").bind(user.id).execute(&state.db).await?;

    // Reaproveita o endpoint de registro de estudo já existente, pra não duplicar a lógica
    // de escada de revisão — chama a mesma função internamente.
    let log_payload = super::study_logs::StudyLogPayload {
        exam_id: timer.exam_id,
        topic_id: timer.topic_id,
        category: timer.category,
        studied_on: chrono::Utc::now().date_naive(),
        minutes: minutes.max(1),
        pages: payload.pages,
        video_minutes: payload.video_minutes,
        questions_total: payload.questions_total,
        questions_correct: payload.questions_correct,
        notes: payload.notes,
        schedule_reviews: payload.schedule_reviews,
    };

    let result = super::study_logs::create_log_internal(&state, user.id, log_payload).await?;
    Ok(Json(result))
}

/// Descarta o cronômetro sem gerar registro nenhum.
async fn cancel(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    sqlx::query("delete from active_timers where user_id = $1").bind(user.id).execute(&state.db).await?;
    Ok(Json(json!({ "message": "cronômetro cancelado" })))
}
