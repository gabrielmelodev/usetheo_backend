use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Question {
    pub id: Uuid,
    pub user_id: Uuid,
    pub exam_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,

    pub banca: Option<String>,
    pub source: Option<String>,
    pub year: Option<i32>,

    pub question_type: String,
    pub statement: String,
    pub alternatives: serde_json::Value,
    pub correct_alternative_id: Option<String>,
    pub explanation: Option<String>,

    pub difficulty: String,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Mesma coisa, mas sem `correct_alternative_id`/`explanation` — usada ao listar questões
/// pra responder, pra não vazar o gabarito antes da hora.
#[derive(Debug, Serialize)]
pub struct QuestionForAnswering {
    pub id: Uuid,
    pub exam_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub banca: Option<String>,
    pub source: Option<String>,
    pub year: Option<i32>,
    pub question_type: String,
    pub statement: String,
    pub alternatives: serde_json::Value,
    pub difficulty: String,
}

impl From<Question> for QuestionForAnswering {
    fn from(q: Question) -> Self {
        QuestionForAnswering {
            id: q.id,
            exam_id: q.exam_id,
            subject_id: q.subject_id,
            topic_id: q.topic_id,
            banca: q.banca,
            source: q.source,
            year: q.year,
            question_type: q.question_type,
            statement: q.statement,
            alternatives: q.alternatives,
            difficulty: q.difficulty,
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct QuestionAttempt {
    pub id: Uuid,
    pub user_id: Uuid,
    pub question_id: Uuid,
    pub selected_alternative_id: Option<String>,
    pub is_correct: bool,
    pub answered_at: DateTime<Utc>,
}
