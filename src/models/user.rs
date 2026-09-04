use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub cpf: String,
    pub country: String,
    pub city: String,
    pub password_hash: String,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub must_change_password: bool,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub role: String,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Representação pública do usuário, é o que a API devolve (nunca o hash de senha ou o segredo TOTP).
#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub city: String,
    pub country: String,
    pub email_verified: bool,
    pub totp_enabled: bool,
    pub role: String,
}

impl From<&User> for UserPublic {
    fn from(u: &User) -> Self {
        UserPublic {
            id: u.id,
            first_name: u.first_name.clone(),
            last_name: u.last_name.clone(),
            email: u.email.clone(),
            city: u.city.clone(),
            country: u.country.clone(),
            email_verified: u.email_verified_at.is_some(),
            totp_enabled: u.totp_enabled,
            role: u.role.clone(),
        }
    }
}
