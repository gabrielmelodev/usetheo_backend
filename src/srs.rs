// Algoritmo SM-2 (SuperMemo 2), o mesmo usado por Anki no modo clássico.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "again" => Some(Rating::Again),
            "hard" => Some(Rating::Hard),
            "good" => Some(Rating::Good),
            "easy" => Some(Rating::Easy),
            _ => None,
        }
    }
}

pub struct SrsResult {
    pub queue: String,
    pub interval_days: f64,
    pub ease_factor: f64,
    pub repetitions: i32,
    pub lapses: i32,
}

/// Aplica a avaliação de uma revisão ao estado atual do cartão e retorna o novo estado.
pub fn apply_review(
    rating: Rating,
    interval_days: f64,
    ease_factor: f64,
    repetitions: i32,
    lapses: i32,
) -> SrsResult {
    match rating {
        Rating::Again => SrsResult {
            queue: "learning".to_string(),
            interval_days: 1.0 / 1440.0 * 10.0, // ~10 minutos
            ease_factor: (ease_factor - 0.2).max(1.3),
            repetitions: 0,
            lapses: lapses + 1,
        },
        Rating::Hard => {
            let new_interval = (interval_days.max(1.0) * 1.2).max(1.0);
            SrsResult {
                queue: "review".to_string(),
                interval_days: new_interval,
                ease_factor: (ease_factor - 0.15).max(1.3),
                repetitions: repetitions + 1,
                lapses,
            }
        }
        Rating::Good => {
            let new_interval = if repetitions == 0 {
                1.0
            } else if repetitions == 1 {
                6.0
            } else {
                (interval_days.max(1.0)) * ease_factor
            };
            SrsResult {
                queue: "review".to_string(),
                interval_days: new_interval,
                ease_factor,
                repetitions: repetitions + 1,
                lapses,
            }
        }
        Rating::Easy => {
            let new_interval = if repetitions == 0 {
                4.0
            } else {
                (interval_days.max(1.0)) * ease_factor * 1.3
            };
            SrsResult {
                queue: "review".to_string(),
                interval_days: new_interval,
                ease_factor: ease_factor + 0.15,
                repetitions: repetitions + 1,
                lapses,
            }
        }
    }
}
