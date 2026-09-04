use crate::state::AppState;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_ATTEMPTS: usize = 5;

/// Limitador simples em memória, por par (email, ip): no máximo 5 tentativas de
/// login por 15 minutos. Suficiente para uma única instância; em produção com
/// múltiplas réplicas isso precisaria virar Redis.
pub fn check_and_record(state: &AppState, email: &str, ip: &str) -> bool {
    let key = format!("{email}|{ip}");
    let now = Instant::now();

    let mut entry = state.login_attempts.entry(key).or_default();
    entry.retain(|t| now.duration_since(*t) < WINDOW);

    if entry.len() >= MAX_ATTEMPTS {
        return false;
    }

    entry.push(now);
    true
}

pub fn reset(state: &AppState, email: &str, ip: &str) {
    let key = format!("{email}|{ip}");
    state.login_attempts.remove(&key);
}
