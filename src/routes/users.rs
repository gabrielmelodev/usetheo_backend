use crate::{auth::middleware::CurrentUser, error::AppResult, models::user::{User, UserPublic}, state::AppState};
use axum::{extract::State, routing::get, Json, Router};

pub fn router() -> Router<AppState> {
    Router::new().route("/me", get(me))
}

async fn me(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<UserPublic>> {
    let db_user: User = sqlx::query_as("select * from users where id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(UserPublic::from(&db_user)))
}
