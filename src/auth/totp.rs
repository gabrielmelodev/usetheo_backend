// Implementação enxuta de TOTP (RFC 6238) sobre HMAC-SHA1 (RFC 4226),
// compatível com Google Authenticator / Authy / etc.

use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

pub fn generate_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &bytes)
}

pub fn otpauth_uri(secret: &str, account_email: &str, issuer: &str) -> String {
    format!(
        "otpauth://totp/{issuer}:{account_email}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits=6&period=30"
    )
}

fn hotp(secret_bytes: &[u8], counter: u64) -> u32 {
    let mut mac = HmacSha1::new_from_slice(secret_bytes).expect("chave hmac de tamanho válido");
    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();

    let offset = (result[result.len() - 1] & 0x0f) as usize;
    let code = ((result[offset] as u32 & 0x7f) << 24)
        | ((result[offset + 1] as u32) << 16)
        | ((result[offset + 2] as u32) << 8)
        | (result[offset + 3] as u32);

    code % 1_000_000
}

fn current_counter() -> u64 {
    (chrono::Utc::now().timestamp() / 30) as u64
}

pub fn generate_code(secret_base32: &str) -> Option<String> {
    let bytes = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, secret_base32)?;
    Some(format!("{:06}", hotp(&bytes, current_counter())))
}

/// Verifica o código aceitando uma janela de +-1 intervalo (30s) para tolerar clock drift.
pub fn verify_code(secret_base32: &str, code: &str) -> bool {
    let bytes = match base32::decode(base32::Alphabet::Rfc4648 { padding: false }, secret_base32) {
        Some(b) => b,
        None => return false,
    };
    let counter = current_counter();

    for window in [-1i64, 0, 1] {
        let c = (counter as i64 + window).max(0) as u64;
        let expected = format!("{:06}", hotp(&bytes, c));
        if expected == code {
            return true;
        }
    }
    false
}
