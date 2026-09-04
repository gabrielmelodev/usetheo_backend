use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Deck {
    pub id: Uuid,
    pub user_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub parent_deck_id: Option<Uuid>,

    pub name: String,
    pub description: Option<String>,
    pub support_text: Option<String>,
    pub about: Option<String>,

    pub target_cards: i32,
    pub card_type: String,
    pub difficulty: String,
    pub color: String,
    pub icon: Option<String>,

    pub show_review_queue: bool,
    pub allow_subdecks: bool,
    pub is_public: bool,
    pub is_official: bool,

    pub anki_name: Option<String>,
    pub imported_from_anki: bool,
    pub review_cards_per_day: Option<i32>,

    pub is_template: bool,
    pub position: i32,
    pub copied_from_deck_id: Option<Uuid>,

    #[serde(rename = "created_at")]
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Deck serializado para a API já com a contagem de cartões (calculada, não persistida).
#[derive(Debug, Serialize)]
pub struct DeckResponse {
    #[serde(flatten)]
    pub deck: Deck,
    pub total_cards: i64,
}
