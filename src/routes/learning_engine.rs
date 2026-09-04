use crate::{auth::middleware::CurrentUser, error::AppResult, state::AppState};
use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new().route("/recommendations", get(recommendations))
}

#[derive(Debug, Serialize)]
struct SubjectSignal {
    subject_id: uuid::Uuid,
    subject_name: String,
    accuracy: f64,
    minutes_last_7_days: i64,
    topics_total: i64,
    topics_studied: i64,
    action: String, // 'reduzir' | 'aumentar' | 'manter'
    reason: String,
}

/// O "Theo Learning Engine": não é um modelo de ML treinado — é um motor de regras
/// transparente e auditável em cima dos dados que o usuário já gerou (desempenho em
/// questões, tempo dedicado, progresso do edital). Decide, por matéria, se deve reduzir,
/// aumentar ou manter o tempo dedicado — o mesmo princípio do item "inteligência adaptativa":
/// nunca elimina uma matéria por completo, só rebalanceia.
async fn recommendations(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<serde_json::Value>> {
    let signals: Vec<(uuid::Uuid, String, i64, i64, i64, i64)> = sqlx::query_as(
        r#"select s.id, s.name,
             coalesce(sum(l.questions_total), 0) as q_total,
             coalesce(sum(l.questions_correct), 0) as q_correct,
             coalesce((select sum(l2.minutes) from study_logs l2
                       join exam_topics t2 on t2.id = l2.topic_id
                       where t2.subject_id = s.id and l2.studied_on >= current_date - interval '7 days'), 0) as minutes_7d,
             count(distinct t.id) as topics_total
           from exam_subjects s
           join exams e on e.id = s.exam_id
           left join exam_topics t on t.subject_id = s.id
           left join study_logs l on l.topic_id = t.id
           where e.user_id = $1
           group by s.id, s.name"#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(signals.len());
    for (subject_id, subject_name, q_total, q_correct, minutes_7d, topics_total) in signals {
        let topics_studied: i64 = sqlx::query_scalar(
            "select count(distinct t.id) from exam_topics t
             join study_logs l on l.topic_id = t.id where t.subject_id = $1",
        )
        .bind(subject_id)
        .fetch_one(&state.db)
        .await?;

        let accuracy = if q_total > 0 { q_correct as f64 / q_total as f64 } else { 0.0 };

        // Regras simples e explicáveis (não uma caixa-preta): >=85% de acerto com volume
        // razoável de questões -> reduzir; <60% -> aumentar; resto -> manter. Nunca chega a
        // zero — é só um sinal de prioridade, a decisão final continua sendo do usuário.
        let (action, reason) = if q_total >= 10 && accuracy >= 0.85 {
            ("reduzir", format!("{}% de acerto em {q_total} questões — já domina bem, dá pra reduzir gradualmente sem abandonar de vez", (accuracy * 100.0).round()))
        } else if q_total >= 10 && accuracy < 0.60 {
            ("aumentar", format!("{}% de acerto em {q_total} questões — merece mais tempo antes de avançar", (accuracy * 100.0).round()))
        } else if q_total < 5 {
            ("manter", "ainda sem questões suficientes pra decidir com confiança — mantém o ritmo atual".to_string())
        } else {
            ("manter", format!("{}% de acerto — dentro da faixa esperada, sem sinal forte pra mudar o ritmo", (accuracy * 100.0).round()))
        };

        out.push(SubjectSignal {
            subject_id,
            subject_name,
            accuracy,
            minutes_last_7_days: minutes_7d,
            topics_total,
            topics_studied,
            action: action.to_string(),
            reason,
        });
    }

    // Prioriza no topo o que precisa de mais atenção — é o que a Home deveria destacar.
    out.sort_by(|a, b| a.accuracy.partial_cmp(&b.accuracy).unwrap());

    Ok(Json(json!({ "subjects": out })))
}
