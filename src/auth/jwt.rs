use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub role: String,
    #[serde(default = "default_purpose")]
    pub purpose: String, // "access" ou "2fa_challenge"
    pub exp: i64,
    pub iat: i64,
}

fn default_purpose() -> String {
    "access".to_string()
}

pub fn issue_access_token(user_id: Uuid, role: &str, secret: &str, ttl_seconds: i64) -> anyhow::Result<String> {
    issue_token(user_id, role, "access", secret, ttl_seconds)
}

/// Token de curta duração emitido após senha correta quando 2FA está habilitado;
/// só serve para completar o login em /auth/2fa/verify, nada mais.
pub fn issue_2fa_challenge_token(user_id: Uuid, secret: &str) -> anyhow::Result<String> {
    issue_token(user_id, "", "2fa_challenge", secret, 5 * 60)
}

fn issue_token(user_id: Uuid, role: &str, purpose: &str, secret: &str, ttl_seconds: i64) -> anyhow::Result<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        purpose: purpose.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(ttl_seconds)).timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(token)
}

pub fn verify_access_token(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let claims = verify_any(token, secret)?;
    if claims.purpose != "access" {
        anyhow::bail!("token com propósito inválido");
    }
    Ok(claims)
}

pub fn verify_2fa_challenge_token(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let claims = verify_any(token, secret)?;
    if claims.purpose != "2fa_challenge" {
        anyhow::bail!("token com propósito inválido");
    }
    Ok(claims)
}

fn verify_any(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}
