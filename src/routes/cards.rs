use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::{
        card::{render_template, Card, CardResponse},
        note::Note,
        note_type::NoteType,
    },
    state::AppState,
};
use axum::{extract::{Query, State}, routing::get, Json, Router};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(index))
}

#[derive(Debug, Deserialize)]
pub struct CardsQuery {
    pub deck_id: Option<Uuid>,
}

async fn index(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<CardsQuery>,
) -> AppResult<Json<Vec<CardResponse>>> {
    let cards: Vec<Card> = if let Some(deck_id) = q.deck_id {
        let owned: i64 = sqlx::query_scalar("select count(*) from decks where id = $1 and user_id = $2")
            .bind(deck_id)
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
        if owned == 0 {
            return Err(AppError::NotFound);
        }

        sqlx::query_as("select * from cards where deck_id = $1 order by inserted_at")
            .bind(deck_id)
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_as("select * from cards where user_id = $1 order by inserted_at")
            .bind(user.id)
            .fetch_all(&state.db)
            .await?
    };

    Ok(Json(render_cards(&state, cards).await?))
}

/// Carrega as notes/note_types necessárias (com cache local para não repetir queries)
/// e devolve cada cartão já com `front`/`back` renderizados.
pub async fn render_cards(state: &AppState, cards: Vec<Card>) -> AppResult<Vec<CardResponse>> {
    let mut notes_cache: HashMap<Uuid, Note> = HashMap::new();
    let mut note_types_cache: HashMap<Uuid, NoteType> = HashMap::new();
    let mut out = Vec::with_capacity(cards.len());

    for card in cards {
        if !notes_cache.contains_key(&card.note_id) {
            let note: Note = sqlx::query_as("select * from notes where id = $1")
                .bind(card.note_id)
                .fetch_one(&state.db)
                .await?;
            notes_cache.insert(card.note_id, note);
        }
        let note = notes_cache.get(&card.note_id).unwrap();

        if !note_types_cache.contains_key(&note.note_type_id) {
            let nt: NoteType = sqlx::query_as("select * from note_types where id = $1")
                .bind(note.note_type_id)
                .fetch_one(&state.db)
                .await?;
            note_types_cache.insert(note.note_type_id, nt);
        }
        let note_type = note_types_cache.get(&note.note_type_id).unwrap();

        let templates = note_type.templates.as_array().cloned().unwrap_or_default();
        let template = templates.get(card.template_index as usize);

        let (front, back) = match template {
            Some(t) => {
                let front_tpl = t.get("front_template").and_then(|v| v.as_str()).unwrap_or("");
                let back_tpl = t.get("back_template").and_then(|v| v.as_str()).unwrap_or("");
                (
                    render_template(front_tpl, &note.fields),
                    render_template(back_tpl, &note.fields),
                )
            }
            None => (String::new(), String::new()),
        };

        out.push(CardResponse {
            id: card.id,
            note_id: card.note_id,
            deck_id: card.deck_id,
            queue: card.queue,
            due: card.due,
            interval_days: card.interval_days,
            ease_factor: card.ease_factor,
            repetitions: card.repetitions,
            lapses: card.lapses,
            front,
            back,
        });
    }

    Ok(out)
}
