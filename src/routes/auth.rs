use crate::{
    audit,
    auth::{jwt, middleware::CurrentUser, password, totp},
    email,
    error::{AppError, AppResult},
    models::user::{User, UserPublic},
    rate_limit,
    state::AppState,
};
use axum::{extract::State, http::HeaderMap, routing::{delete, get, post}, Json, Router};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use validator::Validate;

use super::{client_ip, user_agent};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/2fa/verify", post(verify_2fa_login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/logout-all", post(logout_all))
        .route("/verify-email", post(verify_email))
        .route("/resend-verification", post(resend_verification))
        .route("/forgot-password", post(forgot_password))
        .route("/reset-password", post(reset_password))
        .route("/change-password", post(change_password))
        .route("/2fa/setup", post(setup_2fa))
        .route("/2fa/enable", post(enable_2fa))
        .route("/2fa/disable", post(disable_2fa))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", delete(revoke_session))
}

const CPF_REGEX_DIGITS: usize = 11;

fn only_digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn valid_cpf(cpf: &str) -> bool {
    let digits = only_digits(cpf);
    if digits.len() != CPF_REGEX_DIGITS {
        return false;
    }
    // Todos os dígitos iguais não é um CPF válido (ex: 111.111.111-11).
    !digits.chars().all(|c| c == digits.chars().next().unwrap())
}

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterPayload {
    #[validate(length(min = 1, max = 100))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100))]
    pub last_name: String,
    #[validate(email)]
    pub email: String,
    pub cpf: String,
    #[validate(length(min = 1, max = 100))]
    pub country: String,
    #[validate(length(min = 1, max = 100))]
    pub city: String,
    #[validate(length(min = 8, message = "senha deve ter ao menos 8 caracteres"))]
    pub password: String,
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterPayload>,
) -> AppResult<Json<serde_json::Value>> {
    payload
        .validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    if !valid_cpf(&payload.cpf) {
        return Err(AppError::Validation("CPF inválido".to_string()));
    }
    let cpf_digits = only_digits(&payload.cpf);
    let email = payload.email.trim().to_lowercase();

    let existing = sqlx::query_scalar::<_, i64>(
        "select count(*) from users where email = $1 or cpf = $2",
    )
    .bind(&email)
    .bind(&cpf_digits)
    .fetch_one(&state.db)
    .await?;

    if existing > 0 {
        return Err(AppError::Conflict("e-mail ou CPF já cadastrado".to_string()));
    }

    let password_hash =
        password::hash_password(&payload.password).map_err(AppError::Internal)?;

    let user: User = sqlx::query_as(
        r#"insert into users (first_name, last_name, email, cpf, country, city, password_hash)
           values ($1, $2, $3, $4, $5, $6, $7)
           returning *"#,
    )
    .bind(&payload.first_name)
    .bind(&payload.last_name)
    .bind(&email)
    .bind(&cpf_digits)
    .bind(&payload.country)
    .bind(&payload.city)
    .bind(&password_hash)
    .fetch_one(&state.db)
    .await?;

    send_verification_email(&state, &user).await;

    audit::log(
        &state,
        Some(user.id),
        "user.registered",
        Some(&client_ip(&headers)),
        user_agent(&headers).as_deref(),
        json!({ "email": email }),
    )
    .await;

    Ok(Json(json!({
        "message": "cadastro realizado. verifique seu e-mail para ativar a conta.",
        "user": UserPublic::from(&user),
    })))
}

async fn send_verification_email(state: &AppState, user: &User) {
    let (token, token_hash) = password::generate_opaque_token();
    let expires_at = Utc::now() + Duration::hours(24);

    let _ = sqlx::query(
        "insert into email_verification_tokens (user_id, token_hash, expires_at) values ($1, $2, $3)",
    )
    .bind(user.id)
    .bind(&token_hash)
    .bind(expires_at)
    .execute(&state.db)
    .await;

    let body = email::verification_email_body(&state.config.frontend_url, &token);
    if let Err(e) = email::send_mail(&state.config, &user.email, "Confirme seu e-mail", &body).await {
        tracing::warn!("falha ao enviar e-mail de verificação: {e:?}");
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginPayload {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
struct TokenPair {
    access_token: String,
    refresh_token: String,
}

async fn issue_session(
    state: &AppState,
    user: &User,
    headers: &HeaderMap,
) -> AppResult<TokenPair> {
    let access_token = jwt::issue_access_token(
        user.id,
        &user.role,
        &state.config.jwt_secret,
        state.config.access_token_ttl_seconds,
    )
    .map_err(AppError::Internal)?;

    let (refresh_token, refresh_hash) = password::generate_opaque_token();
    let expires_at = Utc::now() + Duration::days(state.config.refresh_token_ttl_days);

    sqlx::query(
        r#"insert into sessions (user_id, refresh_token_hash, user_agent, ip_address, expires_at)
           values ($1, $2, $3, $4, $5)"#,
    )
    .bind(user.id)
    .bind(&refresh_hash)
    .bind(user_agent(headers))
    .bind(client_ip(headers))
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    Ok(TokenPair {
        access_token,
        refresh_token,
    })
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let ip = client_ip(&headers);
    let email = payload.email.trim().to_lowercase();

    if !rate_limit::check_and_record(&state, &email, &ip) {
        return Err(AppError::RateLimited);
    }

    let user: Option<User> = sqlx::query_as("select * from users where email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;

    let user = match user {
        Some(u) if password::verify_password(&payload.password, &u.password_hash) => u,
        _ => {
            log_login_attempt(&state, &email, &ip, false).await;
            audit::log(&state, None, "login.failed", Some(&ip), user_agent(&headers).as_deref(), json!({ "email": email })).await;
            return Err(AppError::Validation("e-mail ou senha inválidos".to_string()));
        }
    };

    log_login_attempt(&state, &email, &ip, true).await;
    rate_limit::reset(&state, &email, &ip);

    if user.totp_enabled {
        let challenge = jwt::issue_2fa_challenge_token(user.id, &state.config.jwt_secret)
            .map_err(AppError::Internal)?;
        return Ok(Json(json!({
            "requires_2fa": true,
            "challenge_token": challenge,
        })));
    }

    complete_login(&state, &user, &headers).await
}

async fn log_login_attempt(state: &AppState, email: &str, ip: &str, success: bool) {
    let _ = sqlx::query(
        "insert into login_attempts (email, ip_address, success) values ($1, $2, $3)",
    )
    .bind(email)
    .bind(ip)
    .bind(success)
    .execute(&state.db)
    .await;
}

async fn complete_login(state: &AppState, user: &User, headers: &HeaderMap) -> AppResult<Json<serde_json::Value>> {
    let tokens = issue_session(state, user, headers).await?;

    audit::log(
        state,
        Some(user.id),
        "login.success",
        Some(&client_ip(headers)),
        user_agent(headers).as_deref(),
        json!({}),
    )
    .await;

    Ok(Json(json!({
        "access_token": tokens.access_token,
        "refresh_token": tokens.refresh_token,
        "must_change_password": user.must_change_password,
        "user": UserPublic::from(user),
    })))
}

#[derive(Debug, Deserialize)]
pub struct Verify2faLoginPayload {
    pub challenge_token: String,
    pub code: String,
}

async fn verify_2fa_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Verify2faLoginPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let claims = jwt::verify_2fa_challenge_token(&payload.challenge_token, &state.config.jwt_secret)
        .map_err(|_| AppError::Unauthorized)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;

    let user: User = sqlx::query_as("select * from users where id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let secret = user.totp_secret.clone().ok_or(AppError::Unauthorized)?;
    if !totp::verify_code(&secret, &payload.code) {
        return Err(AppError::Validation("código 2FA inválido".to_string()));
    }

    complete_login(&state, &user, &headers).await
}

#[derive(Debug, Deserialize)]
pub struct RefreshPayload {
    pub refresh_token: String,
}

async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RefreshPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let hash = password::hash_token(&payload.refresh_token);

    let session: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"select id, user_id from sessions
           where refresh_token_hash = $1 and revoked_at is null and expires_at > now()"#,
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;

    let (session_id, user_id) = session.ok_or(AppError::Unauthorized)?;

    let user: User = sqlx::query_as("select * from users where id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;

    // Rotaciona o refresh token: revoga o antigo e cria uma nova sessão.
    sqlx::query("update sessions set revoked_at = now() where id = $1")
        .bind(session_id)
        .execute(&state.db)
        .await?;

    let tokens = issue_session(&state, &user, &headers).await?;

    Ok(Json(json!({
        "access_token": tokens.access_token,
        "refresh_token": tokens.refresh_token,
    })))
}

async fn logout(
    State(state): State<AppState>,
    Json(payload): Json<RefreshPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let hash = password::hash_token(&payload.refresh_token);
    sqlx::query("update sessions set revoked_at = now() where refresh_token_hash = $1")
        .bind(&hash)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "message": "logout realizado" })))
}

async fn logout_all(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
        .bind(user.id)
        .execute(&state.db)
        .await?;

    audit::log(&state, Some(user.id), "logout_all", None, None, json!({})).await;

    Ok(Json(json!({ "message": "todas as sessões foram encerradas" })))
}

#[derive(Debug, Deserialize)]
pub struct TokenOnlyPayload {
    pub token: String,
}

async fn verify_email(
    State(state): State<AppState>,
    Json(payload): Json<TokenOnlyPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let hash = password::hash_token(&payload.token);

    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"select id, user_id from email_verification_tokens
           where token_hash = $1 and consumed_at is null and expires_at > now()"#,
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;

    let (token_id, user_id) = row.ok_or(AppError::Validation("token inválido ou expirado".to_string()))?;

    let mut tx = state.db.begin().await?;
    sqlx::query("update email_verification_tokens set consumed_at = now() where id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update users set email_verified_at = now() where id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    audit::log(&state, Some(user_id), "email.verified", None, None, json!({})).await;

    Ok(Json(json!({ "message": "e-mail verificado com sucesso" })))
}

async fn resend_verification(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    let db_user: User = sqlx::query_as("select * from users where id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    if db_user.email_verified_at.is_some() {
        return Ok(Json(json!({ "message": "e-mail já verificado" })));
    }

    send_verification_email(&state, &db_user).await;
    Ok(Json(json!({ "message": "e-mail de verificação reenviado" })))
}

#[derive(Debug, Deserialize)]
pub struct EmailOnlyPayload {
    pub email: String,
}

async fn forgot_password(
    State(state): State<AppState>,
    Json(payload): Json<EmailOnlyPayload>,
) -> AppResult<Json<serde_json::Value>> {
    let email = payload.email.trim().to_lowercase();

    // Resposta genérica sempre, para não revelar se o e-mail existe na base.
    if let Some(user) = sqlx::query_as::<_, User>("select * from users where email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?
    {
        let (token, token_hash) = password::generate_opaque_token();
        let expires_at = Utc::now() + Duration::hours(1);

        sqlx::query(
            "insert into password_reset_tokens (user_id, token_hash, expires_at) values ($1, $2, $3)",
        )
        .bind(user.id)
        .bind(&token_hash)
        .bind(expires_at)
        .execute(&state.db)
        .await?;

        let body = email::password_reset_email_body(&state.config.frontend_url, &token);
        let _ = email::send_mail(&state.config, &user.email, "Redefinição de senha", &body).await;

        audit::log(&state, Some(user.id), "password.reset_requested", None, None, json!({})).await;
    }

    Ok(Json(json!({ "message": "se o e-mail existir, enviaremos instruções para redefinir a senha" })))
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResetPasswordPayload {
    pub token: String,
    #[validate(length(min = 8, message = "senha deve ter ao menos 8 caracteres"))]
    pub password: String,
}

async fn reset_password(
    State(state): State<AppState>,
    Json(payload): Json<ResetPasswordPayload>,
) -> AppResult<Json<serde_json::Value>> {
    payload.validate().map_err(|e| AppError::Validation(e.to_string()))?;

    let hash = password::hash_token(&payload.token);
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"select id, user_id from password_reset_tokens
           where token_hash = $1 and consumed_at is null and expires_at > now()"#,
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;

    let (token_id, user_id) = row.ok_or(AppError::Validation("token inválido ou expirado".to_string()))?;

    let new_hash = password::hash_password(&payload.password).map_err(AppError::Internal)?;

    let mut tx = state.db.begin().await?;
    sqlx::query("update password_reset_tokens set consumed_at = now() where id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update users set password_hash = $1, must_change_password = false where id = $2")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    // Redefinir a senha derruba todas as sessões ativas por segurança.
    sqlx::query("update sessions set revoked_at = now() where user_id = $1 and revoked_at is null")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    audit::log(&state, Some(user_id), "password.reset_completed", None, None, json!({})).await;

    Ok(Json(json!({ "message": "senha redefinida com sucesso" })))
}

#[derive(Debug, Deserialize, Validate)]
pub struct ChangePasswordPayload {
    pub current_password: String,
    #[validate(length(min = 8, message = "senha deve ter ao menos 8 caracteres"))]
    pub new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ChangePasswordPayload>,
) -> AppResult<Json<serde_json::Value>> {
    payload.validate().map_err(|e| AppError::Validation(e.to_string()))?;

    let db_user: User = sqlx::query_as("select * from users where id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    if !password::verify_password(&payload.current_password, &db_user.password_hash) {
        return Err(AppError::Validation("senha atual incorreta".to_string()));
    }

    let new_hash = password::hash_password(&payload.new_password).map_err(AppError::Internal)?;

    sqlx::query("update users set password_hash = $1, must_change_password = false where id = $2")
        .bind(&new_hash)
        .bind(user.id)
        .execute(&state.db)
        .await?;

    audit::log(&state, Some(user.id), "password.changed", None, None, json!({})).await;

    Ok(Json(json!({ "message": "senha alterada com sucesso" })))
}

async fn setup_2fa(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    let db_user: User = sqlx::query_as("select * from users where id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    let secret = totp::generate_secret();

    sqlx::query("update users set totp_secret = $1, totp_enabled = false where id = $2")
        .bind(&secret)
        .bind(user.id)
        .execute(&state.db)
        .await?;

    let uri = totp::otpauth_uri(&secret, &db_user.email, "Theo");

    Ok(Json(json!({ "secret": secret, "otpauth_uri": uri })))
}

#[derive(Debug, Deserialize)]
pub struct CodePayload {
    pub code: String,
}

async fn enable_2fa(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<CodePayload>,
) -> AppResult<Json<serde_json::Value>> {
    let secret: Option<String> = sqlx::query_scalar("select totp_secret from users where id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    let secret = secret.ok_or(AppError::Validation("execute /2fa/setup primeiro".to_string()))?;

    if !totp::verify_code(&secret, &payload.code) {
        return Err(AppError::Validation("código inválido".to_string()));
    }

    sqlx::query("update users set totp_enabled = true where id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await?;

    audit::log(&state, Some(user.id), "2fa.enabled", None, None, json!({})).await;

    Ok(Json(json!({ "message": "2FA habilitado" })))
}

async fn disable_2fa(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<CodePayload>,
) -> AppResult<Json<serde_json::Value>> {
    let secret: Option<String> = sqlx::query_scalar("select totp_secret from users where id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;

    let secret = secret.ok_or(AppError::Validation("2FA não está habilitado".to_string()))?;

    if !totp::verify_code(&secret, &payload.code) {
        return Err(AppError::Validation("código inválido".to_string()));
    }

    sqlx::query("update users set totp_enabled = false, totp_secret = null where id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await?;

    audit::log(&state, Some(user.id), "2fa.disabled", None, None, json!({})).await;

    Ok(Json(json!({ "message": "2FA desabilitado" })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    device_label: Option<String>,
    user_agent: Option<String>,
    ip_address: Option<String>,
    last_used_at: chrono::DateTime<Utc>,
    inserted_at: chrono::DateTime<Utc>,
}

async fn list_sessions(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<SessionRow>>> {
    let sessions: Vec<SessionRow> = sqlx::query_as(
        r#"select id, device_label, user_agent, ip_address, last_used_at, inserted_at
           from sessions
           where user_id = $1 and revoked_at is null and expires_at > now()
           order by last_used_at desc"#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(sessions))
}

async fn revoke_session(
    State(state): State<AppState>,
    user: CurrentUser,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query("update sessions set revoked_at = now() where id = $1 and user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "message": "sessão revogada" })))
}
