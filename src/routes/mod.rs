pub mod admin;
pub mod auth;
pub mod cards;
pub mod dashboard;
pub mod decks;
pub mod exam_dashboard;
pub mod exams;
pub mod folders;
pub mod goals;
pub mod learning_engine;
pub mod materias;
pub mod media;
pub mod note_types;
pub mod notes;
pub mod questions;
pub mod reviews;
pub mod schedule;
pub mod stats;
pub mod study_logs;
pub mod sync;
pub mod timer;
pub mod topic_reviews;
pub mod users;
pub mod ai;

use axum::http::{HeaderMap, HeaderValue};

pub fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .unwrap_or("unknown")
        .trim()
        .to_string()
}

pub fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v: &HeaderValue| v.to_str().ok())
        .map(|s| s.to_string())
}
