use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Exam {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub plan_mode: String, // deadline | continuous | free
    pub exam_date: Option<NaiveDate>,

    pub sprint_days: Option<i32>,

    pub weekly_hours: Option<f64>,
    pub continuous_mode: Option<String>, // semanal | ciclo
    pub max_session_minutes: Option<i32>,

    pub review_intervals_days: Option<serde_json::Value>,
    pub excluded_weekdays: serde_json::Value,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ExamSubject {
    pub id: Uuid,
    pub exam_id: Uuid,
    pub name: String,
    pub weight: i32,
    pub knowledge_factor: f64,
    pub position: i32,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ExamTopic {
    pub id: Uuid,
    pub exam_id: Uuid,
    pub subject_id: Uuid,
    pub parent_topic_id: Option<Uuid>,
    pub number: String,
    pub title: String,
    pub position: i32,
    pub relevance: i32,
    pub in_sprint: bool,
    pub scheduled_date: Option<NaiveDate>,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StudyLog {
    pub id: Uuid,
    pub user_id: Uuid,
    pub exam_id: Uuid,
    pub topic_id: Uuid,
    pub category: String,
    pub studied_on: NaiveDate,
    pub minutes: i32,
    pub pages: Option<i32>,
    pub video_minutes: Option<i32>,
    pub questions_total: Option<i32>,
    pub questions_correct: Option<i32>,
    pub notes: Option<String>,
    pub logged_same_day: bool,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TopicReview {
    pub id: Uuid,
    pub user_id: Uuid,
    pub topic_id: Uuid,
    pub study_log_id: Option<Uuid>,
    pub step: i32,
    pub scheduled_date: NaiveDate,
    pub completed_at: Option<DateTime<Utc>>,
    pub completed_study_log_id: Option<Uuid>,
    pub skipped: bool,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ManualScheduleEntry {
    pub id: Uuid,
    pub user_id: Uuid,
    pub exam_id: Uuid,
    pub weekday: i32,
    pub subject_id: Option<Uuid>,
    pub topic_id: Option<Uuid>,
    pub planned_minutes: Option<i32>,
    pub question_goal: Option<i32>,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
}

impl Exam {
    pub fn review_intervals(&self) -> Vec<i32> {
        self.review_intervals_days
            .as_ref()
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_i64().map(|n| n as i32)).collect())
            .unwrap_or_default()
    }

    pub fn excluded_weekdays_vec(&self) -> Vec<i32> {
        self.excluded_weekdays
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_i64().map(|n| n as i32)).collect())
            .unwrap_or_default()
    }
}
