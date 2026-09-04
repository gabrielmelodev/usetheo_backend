use crate::{
    auth::middleware::CurrentUser,
    error::{AppError, AppResult},
    models::{
        card::Card,
        deck::{Deck, DeckResponse},
        note::Note,
        note_type::NoteType,
    },
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/:id", get(show))
        .route("/:id/clone", post(clone_materia))
}

async fn total_cards(state: &AppState, deck_id: Uuid) -> AppResult<i64> {
    let count: i64 = sqlx::query_scalar("select count(*) from cards where deck_id = $1")
        .bind(deck_id)
        .fetch_one(&state.db)
        .await?;
    Ok(count)
}

/// Catálogo de matérias: decks publicados por administradores, visíveis a
/// qualquer usuário autenticado (não precisa ser dono).
async fn index(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<DeckResponse>>> {
    let decks = sqlx::query_as::<_, Deck>(
        "select * from decks where is_official = true order by name",
    )
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(decks.len());
    for deck in decks {
        let total_cards = total_cards(&state, deck.id).await?;
        out.push(DeckResponse { deck, total_cards });
    }
    Ok(Json(out))
}

async fn show(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<DeckResponse>> {
    let deck = sqlx::query_as::<_, Deck>("select * from decks where id = $1 and is_official = true")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let total_cards = total_cards(&state, deck.id).await?;
    Ok(Json(DeckResponse { deck, total_cards }))
}

/// Copia a matéria inteira (deck + tipos de nota + notes + cards) para o
/// usuário autenticado, com o progresso de SRS zerado — é uma cópia nova e
/// independente, não afeta nem é afetada pelo original.
async fn clone_materia(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let source_deck = sqlx::query_as::<_, Deck>("select * from decks where id = $1 and is_official = true")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let notes: Vec<Note> = sqlx::query_as("select * from notes where deck_id = $1")
        .bind(source_deck.id)
        .fetch_all(&state.db)
        .await?;

    let mut tx = state.db.begin().await?;

    let new_deck: Deck = sqlx::query_as(
        r#"insert into decks
           (user_id, name, description, support_text, about,
            target_cards, card_type, difficulty, color, icon,
            show_review_queue, allow_subdecks, is_public, is_official)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, false, false)
           returning *"#,
    )
    .bind(user.id)
    .bind(&source_deck.name)
    .bind(&source_deck.description)
    .bind(&source_deck.support_text)
    .bind(&source_deck.about)
    .bind(source_deck.target_cards)
    .bind(&source_deck.card_type)
    .bind(&source_deck.difficulty)
    .bind(&source_deck.color)
    .bind(&source_deck.icon)
    .bind(source_deck.show_review_queue)
    .bind(source_deck.allow_subdecks)
    .fetch_one(&mut *tx)
    .await?;

    // Clona cada tipo de nota referenciado uma única vez (cache por id original).
    let mut note_type_map: std::collections::HashMap<Uuid, Uuid> = std::collections::HashMap::new();
    let mut cards_created = 0i64;

    for note in &notes {
        if !note_type_map.contains_key(&note.note_type_id) {
            let source_nt: NoteType = sqlx::query_as("select * from note_types where id = $1")
                .bind(note.note_type_id)
                .fetch_one(&mut *tx)
                .await?;

            let new_nt_id: Uuid = sqlx::query_scalar(
                "insert into note_types (user_id, name, fields, templates) values ($1, $2, $3, $4) returning id",
            )
            .bind(user.id)
            .bind(&source_nt.name)
            .bind(&source_nt.fields)
            .bind(&source_nt.templates)
            .fetch_one(&mut *tx)
            .await?;

            note_type_map.insert(note.note_type_id, new_nt_id);
        }
        let new_note_type_id = note_type_map[&note.note_type_id];

        let new_note_id: Uuid = sqlx::query_scalar(
            "insert into notes (user_id, deck_id, note_type_id, fields, tags) values ($1, $2, $3, $4, $5) returning id",
        )
        .bind(user.id)
        .bind(new_deck.id)
        .bind(new_note_type_id)
        .bind(&note.fields)
        .bind(&note.tags)
        .fetch_one(&mut *tx)
        .await?;

        let source_cards: Vec<Card> = sqlx::query_as("select * from cards where note_id = $1")
            .bind(note.id)
            .fetch_all(&mut *tx)
            .await?;

        for card in source_cards {
            sqlx::query(
                r#"insert into cards (user_id, note_id, deck_id, template_index, queue, due)
                   values ($1, $2, $3, $4, 'new', now())"#,
            )
            .bind(user.id)
            .bind(new_note_id)
            .bind(new_deck.id)
            .bind(card.template_index)
            .execute(&mut *tx)
            .await?;
            cards_created += 1;
        }
    }

    tx.commit().await?;

    Ok(Json(json!({
        "deck_id": new_deck.id,
        "notes_copied": notes.len(),
        "cards_copied": cards_created,
    })))
}
