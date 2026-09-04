use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::exam::{Exam, ExamSubject, ExamTopic},
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::header,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index).post(create))
        .route("/template.xlsx", get(download_template))
        .route("/import-xlsx", post(import_xlsx))
        .route("/:id", get(show).put(update).delete(destroy))
        .route("/:id/subjects", get(list_subjects).post(create_subject))
        .route("/:id/topics", get(list_topics).post(create_topic))
        .route("/:id/topics/bulk", post(bulk_create_topics))
        .route("/subjects/:id", put(update_subject).delete(delete_subject))
        .route("/topics/:id", put(update_topic).delete(delete_topic))
}

async fn find_owned(state: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Exam> {
    sqlx::query_as::<_, Exam>("select * from exams where id = $1 and user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn index(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Exam>>> {
    let exams = sqlx::query_as::<_, Exam>("select * from exams where user_id = $1 order by inserted_at desc")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(exams))
}

async fn show(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Exam>> {
    Ok(Json(find_owned(&state, user.id, id).await?))
}

#[derive(Debug, Deserialize, Default)]
pub struct ExamPayload {
    pub name: String,
    pub plan_mode: String, // deadline | continuous | free
    pub exam_date: Option<chrono::NaiveDate>,
    pub sprint_days: Option<i32>,
    pub weekly_hours: Option<f64>,
    pub continuous_mode: Option<String>,
    pub max_session_minutes: Option<i32>,
    pub review_intervals_days: Option<Vec<i32>>,
    pub excluded_weekdays: Option<Vec<i32>>,
}

fn validate_plan_mode(payload: &ExamPayload) -> AppResult<()> {
    match payload.plan_mode.as_str() {
        "deadline" => {
            if payload.exam_date.is_none() {
                return Err(AppError::Validation("modo 'com prazo certo' exige exam_date".into()));
            }
            if let Some(days) = payload.sprint_days {
                if !(3..=13).contains(&days) {
                    return Err(AppError::Validation("sprint_days deve estar entre 3 e 13".into()));
                }
            }
        }
        "continuous" => {
            if payload.weekly_hours.is_none() {
                return Err(AppError::Validation("modo 'contínuo' exige weekly_hours".into()));
            }
            match payload.continuous_mode.as_deref() {
                Some("semanal") | Some("ciclo") => {}
                _ => return Err(AppError::Validation("continuous_mode deve ser 'semanal' ou 'ciclo'".into())),
            }
        }
        "free" => {}
        _ => return Err(AppError::Validation("plan_mode deve ser deadline, continuous ou free".into())),
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ExamPayload>,
) -> AppResult<Json<Exam>> {
    if payload.name.trim().is_empty() {
        return Err(AppError::Validation("nome é obrigatório".into()));
    }
    validate_plan_mode(&payload)?;

    let exam = sqlx::query_as::<_, Exam>(
        r#"insert into exams
           (user_id, name, plan_mode, exam_date, sprint_days, weekly_hours, continuous_mode,
            max_session_minutes, review_intervals_days, excluded_weekdays)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, coalesce($10, '{}'))
           returning *"#,
    )
    .bind(user.id)
    .bind(&payload.name)
    .bind(&payload.plan_mode)
    .bind(payload.exam_date)
    .bind(payload.sprint_days)
    .bind(payload.weekly_hours)
    .bind(&payload.continuous_mode)
    .bind(payload.max_session_minutes)
    .bind(&payload.review_intervals_days)
    .bind(&payload.excluded_weekdays)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(exam))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<ExamPayload>,
) -> AppResult<Json<Exam>> {
    let existing = find_owned(&state, user.id, id).await?;
    if !payload.plan_mode.trim().is_empty() {
        validate_plan_mode(&payload)?;
    }

    let exam = sqlx::query_as::<_, Exam>(
        r#"update exams set
             name = $1, plan_mode = $2, exam_date = $3, sprint_days = $4,
             weekly_hours = $5, continuous_mode = $6, max_session_minutes = $7,
             review_intervals_days = $8, excluded_weekdays = coalesce($9, excluded_weekdays),
             updated_at = now()
           where id = $10
           returning *"#,
    )
    .bind(if payload.name.trim().is_empty() { existing.name } else { payload.name })
    .bind(if payload.plan_mode.trim().is_empty() { existing.plan_mode } else { payload.plan_mode })
    .bind(payload.exam_date.or(existing.exam_date))
    .bind(payload.sprint_days.or(existing.sprint_days))
    .bind(payload.weekly_hours.or(existing.weekly_hours))
    .bind(payload.continuous_mode.or(existing.continuous_mode))
    .bind(payload.max_session_minutes.or(existing.max_session_minutes))
    .bind(payload.review_intervals_days.or(existing.review_intervals_days))
    .bind(payload.excluded_weekdays)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(exam))
}

async fn destroy(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    find_owned(&state, user.id, id).await?;
    sqlx::query("delete from exams where id = $1").bind(id).execute(&state.db).await?;
    Ok(Json(serde_json::json!({ "message": "edital removido" })))
}

// ---------------- Matérias ----------------

async fn list_subjects(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Vec<ExamSubject>>> {
    find_owned(&state, user.id, id).await?;
    let subjects = sqlx::query_as::<_, ExamSubject>(
        "select * from exam_subjects where exam_id = $1 order by position, name",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(subjects))
}

#[derive(Debug, Deserialize)]
pub struct SubjectPayload {
    pub name: String,
    pub weight: Option<i32>,
    pub knowledge_factor: Option<f64>,
}

async fn create_subject(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<SubjectPayload>,
) -> AppResult<Json<ExamSubject>> {
    find_owned(&state, user.id, id).await?;

    let subject = sqlx::query_as::<_, ExamSubject>(
        "insert into exam_subjects (exam_id, name, weight, knowledge_factor) values ($1, $2, coalesce($3, 3), coalesce($4, 1.0)) returning *",
    )
    .bind(id)
    .bind(&payload.name)
    .bind(payload.weight)
    .bind(payload.knowledge_factor)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(subject))
}

async fn update_subject(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<SubjectPayload>,
) -> AppResult<Json<ExamSubject>> {
    let owned: i64 = sqlx::query_scalar(
        "select count(*) from exam_subjects s join exams e on e.id = s.exam_id where s.id = $1 and e.user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if owned == 0 {
        return Err(AppError::NotFound);
    }

    let subject = sqlx::query_as::<_, ExamSubject>(
        r#"update exam_subjects set
             name = $1, weight = coalesce($2, weight), knowledge_factor = coalesce($3, knowledge_factor), updated_at = now()
           where id = $4 returning *"#,
    )
    .bind(&payload.name)
    .bind(payload.weight)
    .bind(payload.knowledge_factor)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(subject))
}

async fn delete_subject(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    let owned: i64 = sqlx::query_scalar(
        "select count(*) from exam_subjects s join exams e on e.id = s.exam_id where s.id = $1 and e.user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if owned == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("delete from exam_subjects where id = $1").bind(id).execute(&state.db).await?;
    Ok(Json(serde_json::json!({ "message": "matéria removida" })))
}

// ---------------- Tópicos ----------------

async fn list_topics(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<Vec<ExamTopic>>> {
    find_owned(&state, user.id, id).await?;
    let topics = sqlx::query_as::<_, ExamTopic>(
        "select * from exam_topics where exam_id = $1 order by position",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(topics))
}

#[derive(Debug, Deserialize)]
pub struct TopicPayload {
    pub subject_id: Uuid,
    pub parent_topic_id: Option<Uuid>,
    pub number: String,
    pub title: String,
    pub relevance: Option<i32>,
    pub in_sprint: Option<bool>,
}

async fn create_topic(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<TopicPayload>,
) -> AppResult<Json<ExamTopic>> {
    find_owned(&state, user.id, id).await?;
    let topic = insert_topic(&state, id, &payload, 0).await?;
    Ok(Json(topic))
}

async fn insert_topic(state: &AppState, exam_id: Uuid, payload: &TopicPayload, position: i32) -> AppResult<ExamTopic> {
    let topic = sqlx::query_as::<_, ExamTopic>(
        r#"insert into exam_topics (exam_id, subject_id, parent_topic_id, number, title, position, relevance, in_sprint)
           values ($1, $2, $3, $4, $5, $6, coalesce($7, 2), coalesce($8, false))
           returning *"#,
    )
    .bind(exam_id)
    .bind(payload.subject_id)
    .bind(payload.parent_topic_id)
    .bind(&payload.number)
    .bind(&payload.title)
    .bind(position)
    .bind(payload.relevance)
    .bind(payload.in_sprint)
    .fetch_one(&state.db)
    .await?;
    Ok(topic)
}

async fn bulk_create_topics(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payloads): Json<Vec<TopicPayload>>,
) -> AppResult<Json<Vec<ExamTopic>>> {
    find_owned(&state, user.id, id).await?;

    let mut out = Vec::with_capacity(payloads.len());
    for (i, payload) in payloads.iter().enumerate() {
        out.push(insert_topic(&state, id, payload, i as i32).await?);
    }
    Ok(Json(out))
}

async fn update_topic(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<TopicPayload>,
) -> AppResult<Json<ExamTopic>> {
    let owned: i64 = sqlx::query_scalar(
        "select count(*) from exam_topics t join exams e on e.id = t.exam_id where t.id = $1 and e.user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if owned == 0 {
        return Err(AppError::NotFound);
    }

    let topic = sqlx::query_as::<_, ExamTopic>(
        r#"update exam_topics set
             subject_id = $1, parent_topic_id = $2, number = $3, title = $4,
             relevance = coalesce($5, relevance), in_sprint = coalesce($6, in_sprint), updated_at = now()
           where id = $7 returning *"#,
    )
    .bind(payload.subject_id)
    .bind(payload.parent_topic_id)
    .bind(&payload.number)
    .bind(&payload.title)
    .bind(payload.relevance)
    .bind(payload.in_sprint)
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(topic))
}

async fn delete_topic(State(state): State<AppState>, user: CurrentUser, Path(id): Path<Uuid>) -> AppResult<Json<serde_json::Value>> {
    let owned: i64 = sqlx::query_scalar(
        "select count(*) from exam_topics t join exams e on e.id = t.exam_id where t.id = $1 and e.user_id = $2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if owned == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("delete from exam_topics where id = $1").bind(id).execute(&state.db).await?;
    Ok(Json(serde_json::json!({ "message": "tópico removido" })))
}

// ---------------- Planilha (.xlsx) ----------------

/// Modelo pronto para o usuário preencher: linha 1 = nome do edital, linha 2 = cabeçalhos,
/// demais linhas = Matéria | Ponto | Peso (1-5) | Observação.
async fn download_template() -> AppResult<impl IntoResponse> {
    use rust_xlsxwriter::Workbook;

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();

    sheet
        .write_string(0, 0, "Cargo / Órgão / Ano")
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    sheet
        .write_string(1, 0, "Matéria")
        .and_then(|s| s.write_string(1, 1, "Ponto"))
        .and_then(|s| s.write_string(1, 2, "Peso (1-5)"))
        .and_then(|s| s.write_string(1, 3, "Observação"))
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    sheet
        .write_string(2, 0, "Direito Administrativo")
        .and_then(|s| s.write_string(2, 1, "1. Ato administrativo: conceito e atributos"))
        .and_then(|s| s.write_number(2, 2, 4.0))
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    let bytes = workbook
        .save_to_buffer()
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"modelo-edital.xlsx\""),
        ],
        Bytes::from(bytes),
    ))
}

static LEADING_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*([\d]+(?:\.[\d]+)*)\.?\s+(.*)$").unwrap());

/// Recebe um .xlsx no formato do modelo e cria o edital inteiro (matérias + tópicos numerados).
async fn import_xlsx(
    State(state): State<AppState>,
    user: CurrentUser,
    mut multipart: Multipart,
) -> AppResult<Json<Exam>> {
    use calamine::{open_workbook_from_rs, DataType, Reader, Xlsx};
    use std::io::Cursor;

    let mut file_bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::Validation(e.to_string()))? {
        if field.name() == Some("file") {
            file_bytes = Some(field.bytes().await.map_err(|e| AppError::Validation(e.to_string()))?.to_vec());
        }
    }
    let bytes = file_bytes.ok_or_else(|| AppError::Validation("arquivo .xlsx (campo 'file') é obrigatório".into()))?;

    let cursor = Cursor::new(bytes);
    let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor)
        .map_err(|e| AppError::Validation(format!("não foi possível ler o .xlsx: {e}")))?;

    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| AppError::Validation("planilha vazia".into()))?;

    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| AppError::Validation(format!("não foi possível ler a planilha: {e}")))?;

    let mut rows = range.rows();

    let exam_name = rows
        .next()
        .and_then(|r| r.first())
        .map(|c| c.to_string())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::Validation("linha 1 (nome do edital) vazia".into()))?;

    let _headers = rows.next(); // linha 2, só cabeçalhos, ignorada no parse

    let exam = sqlx::query_as::<_, Exam>(
        "insert into exams (user_id, name, plan_mode, excluded_weekdays) values ($1, $2, 'free', '{}') returning *",
    )
    .bind(user.id)
    .bind(&exam_name)
    .fetch_one(&state.db)
    .await?;

    let mut subject_ids: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();
    let mut subject_position = 0i32;
    let mut topic_position = 0i32;

    for row in rows {
        let subject_name = row.first().map(|c| c.to_string()).unwrap_or_default();
        let point_raw = row.get(1).map(|c| c.to_string()).unwrap_or_default();
        if subject_name.trim().is_empty() || point_raw.trim().is_empty() {
            continue;
        }
        let weight: i32 = row
            .get(2)
            .and_then(|c| c.get_float())
            .map(|f| f.round() as i32)
            .unwrap_or(3);

        let subject_id = match subject_ids.get(subject_name.trim()) {
            Some(id) => *id,
            None => {
                let s = sqlx::query_as::<_, ExamSubject>(
                    "insert into exam_subjects (exam_id, name, weight, position) values ($1, $2, $3, $4) returning *",
                )
                .bind(exam.id)
                .bind(subject_name.trim())
                .bind(weight.clamp(1, 5))
                .bind(subject_position)
                .fetch_one(&state.db)
                .await?;
                subject_position += 1;
                subject_ids.insert(subject_name.trim().to_string(), s.id);
                s.id
            }
        };

        let (number, title) = match LEADING_NUMBER.captures(point_raw.trim()) {
            Some(caps) => (caps[1].to_string(), caps[2].to_string()),
            None => (format!("{}", topic_position + 1), point_raw.trim().to_string()),
        };

        sqlx::query(
            "insert into exam_topics (exam_id, subject_id, number, title, position) values ($1, $2, $3, $4, $5)",
        )
        .bind(exam.id)
        .bind(subject_id)
        .bind(&number)
        .bind(&title)
        .bind(topic_position)
        .execute(&state.db)
        .await?;
        topic_position += 1;
    }

    Ok(Json(exam))
}
