use crate::{auth::jwt, error::AppError, state::AppState};
use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use uuid::Uuid;

/// Usuário autenticado, extraído e validado a partir do header `Authorization: Bearer <token>`.
/// Usar como argumento de handler: `async fn handler(user: CurrentUser, ...)`.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub role: String,
}

/// Igual a `CurrentUser`, mas rejeita com 403 quem não for admin.
/// Uso: `async fn handler(admin: RequireAdmin, ...)` — `admin.0` dá acesso ao `CurrentUser`.
#[derive(Debug, Clone)]
pub struct RequireAdmin(pub CurrentUser);

#[async_trait]
impl<S> FromRequestParts<S> for CurrentUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let header_token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        // EventSource (usado em /sync/watch) não permite mandar headers
        // customizados, então para essa rota aceitamos o token via
        // `?access_token=` na query string além do header normal.
        let query_token = parts
            .uri
            .query()
            .and_then(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .find(|(k, _)| k == "access_token")
                    .map(|(_, v)| v.into_owned())
            });

        let token = header_token.or(query_token).ok_or(AppError::Unauthorized)?;

        let claims = jwt::verify_access_token(&token, &app_state.config.jwt_secret)
            .map_err(|_| AppError::Unauthorized)?;

        let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;

        Ok(CurrentUser {
            id,
            role: claims.role,
        })
    }
}

/// Igual a `CurrentUser`, mas rejeita (403) quem não tem `role = "admin"`.
/// Usar em handlers administrativos: `async fn handler(admin: AdminUser, ...)`.
#[derive(Debug, Clone)]
pub struct AdminUser(pub CurrentUser);

#[async_trait]
impl<S> FromRequestParts<S> for AdminUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let current = CurrentUser::from_request_parts(parts, state).await?;
        if current.role != "admin" {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(current))
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for RequireAdmin
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        if user.role != "admin" {
            return Err(AppError::Forbidden);
        }
        Ok(RequireAdmin(user))
    }
}
