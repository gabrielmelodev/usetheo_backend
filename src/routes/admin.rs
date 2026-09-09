use crate::{
    auth::middleware::AdminUser,
    error::{AppError, AppResult},
    models::{
        deck::{Deck, DeckResponse},
        user::{User, UserPublic},
    },
    state::AppState,
};

use axum::{
    extract::{Path, State},
    routing::{delete, get, patch},
    Json, Router,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::decks::{
    admin_set_template,
    ensure_template_folder,
    DeckPayload,
};

// ============================================================
// ROUTER
// ============================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats", get(platform_stats))
        .route("/users", get(list_users))
        .route("/users/:id/role", patch(set_role))
        .route(
            "/decks",
            get(list_all_decks).post(create_native_deck),
        )
        .route("/decks/:id", delete(delete_any_deck))
        .route(
            "/decks/:id/template",
            patch(admin_set_template),
        )
}

// ============================================================
// ESTATÍSTICAS DA PLATAFORMA
// ============================================================

#[derive(Debug, Serialize, sqlx::FromRow)]
struct PlatformStats {
    total_users: i64,
    total_decks: i64,
    total_template_decks: i64,
    total_cards: i64,
    total_exams: i64,
}

async fn platform_stats(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<PlatformStats>> {
    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.remote_db)
        .await?;
    let (total_decks, total_template_decks, total_cards, total_exams): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM decks), (SELECT COUNT(*) FROM decks WHERE is_template = TRUE), (SELECT COUNT(*) FROM cards), (SELECT COUNT(*) FROM exams)"
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(PlatformStats { total_users, total_decks, total_template_decks, total_cards, total_exams }))
}

// ============================================================
// USUÁRIOS
// ============================================================

async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<Vec<UserPublic>>> {
    let users: Vec<User> = sqlx::query_as(
        r#"
        SELECT *
        FROM users
        ORDER BY inserted_at DESC
        "#,
    )
    .fetch_all(&state.remote_db)
    .await?;

    let users = users
        .iter()
        .map(UserPublic::from)
        .collect();

    Ok(Json(users))
}

// ============================================================
// ALTERAÇÃO DE ROLE
// ============================================================

#[derive(Debug, Deserialize)]
pub struct SetRolePayload {
    pub role: String,
}

#[derive(Debug, Serialize)]
struct RoleResponse {
    message: String,
    user_id: Uuid,
    role: String,
}

async fn set_role(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<SetRolePayload>,
) -> AppResult<Json<RoleResponse>> {
    let role = payload.role.trim().to_lowercase();

    // --------------------------------------------------------
    // ROLES PERMITIDAS
    // --------------------------------------------------------

    if !matches!(role.as_str(), "user" | "admin") {
        return Err(AppError::Validation(
            "papel inválido. Use 'user' ou 'admin'.".into(),
        ));
    }

    // --------------------------------------------------------
    // EVITA ALTERAR O PRÓPRIO USUÁRIO
    // --------------------------------------------------------

    if admin.0.id == id {
        return Err(AppError::Validation(
            "não é permitido alterar o próprio papel de administrador.".into(),
        ));
    }

    // --------------------------------------------------------
    // ATUALIZA SOMENTE SE O USUÁRIO EXISTIR
    // --------------------------------------------------------

    let result = sqlx::query(
        r#"
        UPDATE users
        SET
            role = $1,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $2
        "#,
    )
    .bind(&role)
    .bind(id)
    .execute(&state.remote_db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(Json(RoleResponse {
        message: "papel atualizado com sucesso".into(),
        user_id: id,
        role,
    }))
}

// ============================================================
// DECKS ADMIN
// ============================================================

#[derive(Debug, Serialize)]
struct AdminDeckRow {
    #[serde(flatten)]
    deck: DeckResponse,

    owner_email: String,
}

// ============================================================
// LINHA AUXILIAR PARA CONTAGEM DOS CARDS
// ============================================================

#[derive(Debug, sqlx::FromRow)]
struct DeckAdminInfo {
    deck_id: Uuid,
    total_cards: i64,
}

// ============================================================
// LISTAR TODOS OS DECKS
// ============================================================

async fn list_all_decks(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<Vec<AdminDeckRow>>> {
    // --------------------------------------------------------
    // 1. Busca todos os decks
    //
    // Aqui usamos exatamente o mesmo padrão que já funciona
    // no seu módulo de decks:
    //
    // query_as::<_, Deck>()
    // --------------------------------------------------------

    let decks: Vec<Deck> = sqlx::query_as::<_, Deck>(
        r#"
        SELECT *
        FROM decks
        ORDER BY inserted_at DESC
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    // --------------------------------------------------------
    // 2. Busca proprietário + quantidade de cards
    //
    // Uma única consulta para todos os decks.
    // --------------------------------------------------------

    let admin_info: Vec<DeckAdminInfo> = sqlx::query_as::<_, DeckAdminInfo>(
        r#"
        SELECT d.id AS deck_id, COUNT(c.id) AS total_cards
        FROM decks d
        LEFT JOIN cards c ON c.deck_id = d.id
        GROUP BY d.id
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let remote_users: Vec<User> = sqlx::query_as("SELECT * FROM users")
        .fetch_all(&state.remote_db)
        .await?;
    let emails_by_user: std::collections::HashMap<Uuid, String> = remote_users
        .into_iter()
        .map(|u| (u.id, u.email))
        .collect();

    // --------------------------------------------------------
    // 3. Indexa as informações por deck_id
    // --------------------------------------------------------

    use std::collections::HashMap;

    let info_by_deck: HashMap<Uuid, DeckAdminInfo> = admin_info
        .into_iter()
        .map(|info| (info.deck_id, info))
        .collect();

    // --------------------------------------------------------
    // 4. Monta resposta final
    // --------------------------------------------------------

    let result = decks
        .into_iter()
        .map(|deck| {
            let info = info_by_deck.get(&deck.id);
            let owner_id = deck.user_id;

            AdminDeckRow {
                deck: DeckResponse {
                    deck,
                    total_cards: info
                        .map(|value| value.total_cards)
                        .unwrap_or(0),
                },

                owner_email: emails_by_user
                    .get(&owner_id)
                    .cloned()
                    .unwrap_or_else(|| "desconhecido".to_string()),
            }
        })
        .collect();

    Ok(Json(result))
}

// ============================================================
// CRIAR DECK NATIVO DO THEO
// ============================================================

async fn create_native_deck(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(payload): Json<DeckPayload>,
) -> AppResult<Json<DeckResponse>> {
    // --------------------------------------------------------
    // VALIDAÇÃO
    // --------------------------------------------------------

    let name = payload.name.trim();

    if name.is_empty() {
        return Err(AppError::Validation(
            "nome é obrigatório".into(),
        ));
    }

    if name.len() > 200 {
        return Err(AppError::Validation(
            "nome do deck deve ter no máximo 200 caracteres."
                .into(),
        ));
    }

    // --------------------------------------------------------
    // GARANTE PASTA DE TEMPLATES
    // --------------------------------------------------------

    let template_folder_id =
        ensure_template_folder(
            &state,
            admin.0.id,
        )
        .await?;

    // --------------------------------------------------------
    // CRIA DECK NATIVO
    // --------------------------------------------------------

    let deck = sqlx::query_as::<_, Deck>(
        r#"
        INSERT INTO decks (
            user_id,
            name,
            description,
            support_text,
            about,
            folder_id,
            target_cards,
            card_type,
            difficulty,
            color,
            icon,
            show_review_queue,
            allow_subdecks,
            is_public,
            is_template,
            is_official
        )
        VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            COALESCE($7, 20),
            COALESCE($8, 'basic'),
            COALESCE($9, 'medio'),
            COALESCE($10, '#8b5cf6'),
            $11,
            COALESCE($12, TRUE),
            COALESCE($13, TRUE),
            COALESCE($14, FALSE),
            TRUE,
            TRUE
        )
        RETURNING *
        "#,
    )
    .bind(admin.0.id)
    .bind(name)
    .bind(&payload.description)
    .bind(&payload.support_text)
    .bind(&payload.about)
    .bind(template_folder_id)
    .bind(payload.target_cards)
    .bind(&payload.card_type)
    .bind(&payload.difficulty)
    .bind(&payload.color)
    .bind(&payload.icon)
    .bind(payload.show_review_queue)
    .bind(payload.allow_subdecks)
    .bind(payload.is_public)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(DeckResponse {
        deck,
        total_cards: 0,
    }))
}

// ============================================================
// EXCLUIR DECK
// ============================================================

async fn delete_any_deck(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let result = sqlx::query(
        r#"
        DELETE FROM decks
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(Json(json!({
        "message": "deck removido pelo admin",
        "deck_id": id
    })))
}