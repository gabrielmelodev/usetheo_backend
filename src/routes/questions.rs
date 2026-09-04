use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::question::{Question, QuestionAttempt, QuestionForAnswering},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index).post(create))
        .route("/stats", get(stats))
        .route("/wrong", get(wrong))
        .route("/:id", get(show).put(update).delete(destroy))
        .route("/:id/answer", post(answer))
}

#[derive(Debug, Deserialize)]
pub struct QuestionsQuery {
    pub exam_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub banca: Option<String>,
    pub difficulty: Option<String>,
    /// Se true, esconde o gabarito (uso ao montar uma lista pra responder).
    #[serde(default)]
    pub for_answering: bool,
}

async fn index(State(state): State<AppState>, user: CurrentUser, Query(q): Query<QuestionsQuery>) -> AppResult<Json<serde_json::Value>> {
    let questions: Vec<Question> = sqlx::query_as(
        r#"select * from questions
           where user_id = $1
             and ($2::uuid is null or exam_id = $2)
             and ($3::uuid is null or subject_id = $3)
             and ($4::uuid is null or topic_id = $4)
             and ($5::text is null or banca ilike '%' || $5 || '%')
             and ($6::text is null or difficulty = $6)
           order by inserted_at desc"#,
    )
    .bind(user.id)
    .bind(q.exam_id)
    .bind(q.subject_id)
    .bind(q.topic_id)
    .bind(&q.banca)
    .bind(&q.difficulty)
    .fetch_all(&state.db)
    .await?;

    if q.for_answering {
        let hidden: Vec<QuestionForAnswering> = questions.into_iter().map(Into::into).collect();
        Ok(Json(json!(hidden)))
    } else {
        Ok(Json(json!(questions)))
    }
}

async fn find_owned(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Question> {
    sqlx::query_as::<_, Question>("select * from questions where id = $1 and user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn show(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Question>> {
    Ok(Json(find_owned(&state, user.id, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct QuestionPayload {
    pub exam_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub banca: Option<String>,
    pub source: Option<String>,
    pub year: Option<i32>,
    #[serde(default = "default_type")]
    pub question_type: String,
    pub statement: String,
    #[serde(default)]
    pub alternatives: Vec<serde_json::Value>,
    pub correct_alternative_id: Option<String>,
    pub explanation: Option<String>,
    #[serde(default = "default_difficulty")]
    pub difficulty: String,
}

fn default_type() -> String {
    "multiple_choice".to_string()
}

fn default_difficulty() -> String {
    "medio".to_string()
}

fn validate(payload: &QuestionPayload) -> AppResult<()> {
    if payload.statement.trim().is_empty() {
        return Err(AppError::Validation("enunciado é obrigatório".into()));
    }
    if !["multiple_choice", "true_false", "open"].contains(&payload.question_type.as_str()) {
        return Err(AppError::Validation("question_type deve ser multiple_choice, true_false ou open".into()));
    }
    if payload.question_type == "multiple_choice" && payload.alternatives.len() < 2 {
        return Err(AppError::Validation("múltipla escolha exige ao menos 2 alternativas".into()));
    }
    if payload.question_type != "open" && payload.correct_alternative_id.is_none() {
        return Err(AppError::Validation("correct_alternative_id é obrigatório (exceto para questões abertas)".into()));
    }
    if !["facil", "medio", "dificil"].contains(&payload.difficulty.as_str()) {
        return Err(AppError::Validation("difficulty deve ser facil, medio ou dificil".into()));
    }
    Ok(())
}

async fn create(State(state): State<AppState>, user: CurrentUser, Json(payload): Json<QuestionPayload>) -> AppResult<Json<Question>> {
    validate(&payload)?;

    let alternatives = serde_json::to_value(&payload.alternatives).unwrap();

    let question = sqlx::query_as::<_, Question>(
        r#"insert into questions
           (user_id, exam_id, subject_id, topic_id, banca, source, year,
            question_type, statement, alternatives, correct_alternative_id, explanation, difficulty)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
           returning *"#,
    )
    .bind(user.id)
    .bind(payload.exam_id)
    .bind(payload.subject_id)
    .bind(payload.topic_id)
    .bind(&payload.banca)
    .bind(&payload.source)
    .bind(payload.year)
    .bind(&payload.question_type)
    .bind(&payload.statement)
    .bind(&alternatives)
    .bind(&payload.correct_alternative_id)
    .bind(&payload.explanation)
    .bind(&payload.difficulty)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(question))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<QuestionPayload>,
) -> AppResult<Json<Question>> {
    validate(&payload)?;
    find_owned(&state, user.id, id).await?;

    let alternatives = serde_json::to_value(&payload.alternatives).unwrap();

    let question = sqlx::query_as::<_, Question>(
        r#"update questions set
             exam_id = $1, subject_id = $2, topic_id = $3, banca = $4, source = $5, year = $6,
             question_type = $7, statement = $8, alternatives = $9, correct_alternative_id = $10,
             explanation = $11, difficulty = $12, updated_at = now()
           where id = $13 returning *"#,
    )
    .bind(payload.exam_id)
    .bind(payload.subject_id)
    .bind(payload.topic_id)
    .bind(&payload.banca)
    .bind(&payload.source)
    .bind(payload.year)
    .bind(&payload.question_type)
    .bind(&payload.statement)
    .bind(&alternatives)
    .bind(&payload.correct_alternative_id)
    .bind(&payload.explanation)
    .bind(&payload.difficulty)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(question))
}

async fn destroy(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    find_owned(&state, user.id, id).await?;
    sqlx::query("delete from questions where id = $1").bind(id).execute(&state.db).await?;
    Ok(Json(json!({ "message": "questão removida" })))
}

#[derive(Debug, Deserialize)]
pub struct AnswerPayload {
    /// Id da alternativa escolhida, ou "true"/"false" para certo-errado. Nulo para questões abertas
    /// (nesse caso não dá pra corrigir automaticamente — fica registrado como não avaliada).
    pub selected_alternative_id: Option<String>,
}

async fn answer(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<AnswerPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let question = find_owned(&state, user.id, id).await?;

    let is_correct = match (&question.correct_alternative_id, &payload.selected_alternative_id) {
        (Some(correct), Some(selected)) => correct == selected,
        _ => false,
    };

    let attempt: QuestionAttempt = sqlx::query_as(
        "insert into question_attempts (user_id, question_id, selected_alternative_id, is_correct)
         values ($1, $2, $3, $4) returning *",
    )
    .bind(user.id)
    .bind(id)
    .bind(&payload.selected_alternative_id)
    .bind(is_correct)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({
        "attempt": attempt,
        "is_correct": is_correct,
        "correct_alternative_id": question.correct_alternative_id,
        "explanation": question.explanation,
    })))
}

/// Caderno de erros (base): últimas questões em que a tentativa mais recente do usuário
/// foi errada.
async fn wrong(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Question>>> {
    let questions: Vec<Question> = sqlx::query_as(
        r#"select q.* from questions q
           where q.user_id = $1
             and exists (
               select 1 from question_attempts a
               where a.question_id = q.id and a.user_id = $1 and a.is_correct = false
               and a.answered_at = (select max(a2.answered_at) from question_attempts a2 where a2.question_id = q.id and a2.user_id = $1)
             )
           order by q.updated_at desc"#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(questions))
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    pub exam_id: Option<Uuid>,
}

/// Desempenho em questões: total, % de acerto geral, e a quebra por matéria e por banca —
/// a base do item "Questões" do prompt.
async fn stats(State(state): State<AppState>, user: CurrentUser, Query(q): Query<StatsQuery>) -> AppResult<Json<serde_json::Value>> {
    let (total, correct): (i64, i64) = sqlx::query_as(
        r#"select count(*), count(*) filter (where a.is_correct)
           from question_attempts a
           join questions qq on qq.id = a.question_id
           where a.user_id = $1 and ($2::uuid is null or qq.exam_id = $2)"#,
    )
    .bind(user.id)
    .bind(q.exam_id)
    .fetch_one(&state.db)
    .await?;

    let by_subject: Vec<(Option<String>, i64, i64)> = sqlx::query_as(
        r#"select s.name, count(*), count(*) filter (where a.is_correct)
           from question_attempts a
           join questions qq on qq.id = a.question_id
           left join exam_subjects s on s.id = qq.subject_id
           where a.user_id = $1 and ($2::uuid is null or qq.exam_id = $2)
           group by s.name"#,
    )
    .bind(user.id)
    .bind(q.exam_id)
    .fetch_all(&state.db)
    .await?;

    let by_banca: Vec<(Option<String>, i64, i64)> = sqlx::query_as(
        r#"select qq.banca, count(*), count(*) filter (where a.is_correct)
           from question_attempts a
           join questions qq on qq.id = a.question_id
           where a.user_id = $1 and ($2::uuid is null or qq.exam_id = $2)
           group by qq.banca"#,
    )
    .bind(user.id)
    .bind(q.exam_id)
    .fetch_all(&state.db)
    .await?;

    let fmt = |rows: Vec<(Option<String>, i64, i64)>| -> Vec<serde_json::Value> {
        rows.into_iter()
            .map(|(name, total, correct)| {
                json!({
                    "name": name.unwrap_or_else(|| "Sem categoria".to_string()),
                    "total": total,
                    "correct": correct,
                    "accuracy": if total > 0 { correct as f64 / total as f64 } else { 0.0 },
                })
            })
            .collect()
    };

    Ok(Json(json!({
        "total": total,
        "correct": correct,
        "accuracy": if total > 0 { correct as f64 / total as f64 } else { 0.0 },
        "by_subject": fmt(by_subject),
        "by_banca": fmt(by_banca),
    })))
}
