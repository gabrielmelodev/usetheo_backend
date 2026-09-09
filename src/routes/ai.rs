use crate::{
    auth::middleware::CurrentUser,
    error::AppResult,
    service::planalto::PlanaltoService,
    state::AppState,
};

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendations", get(recommendations))
        .route("/chat", post(chat))
}

// ============================================================
// RECOMMENDATIONS
// ============================================================

#[derive(Debug, Serialize)]
struct SubjectSignal {
    subject_id: Uuid,
    subject_name: String,
    accuracy: f64,
    minutes_last_7_days: i64,
    topics_total: i64,
    topics_studied: i64,
    action: String,
    reason: String,
}

// ============================================================
// CHAT
// ============================================================

#[derive(Debug, Deserialize)]
struct ChatRequest {
    message: String,

    #[serde(default)]
    conversation: Vec<ChatMessage>,

    #[serde(default)]
    source_ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    message: String,
    conversation_id: Option<Uuid>,

    #[serde(default)]
    sources: Vec<ChatSource>,
}

#[derive(Debug, Serialize)]
struct ChatSource {
    title: String,
    url: String,
    source: String,
}

// ============================================================
// MEMÓRIA DO THEO
// ============================================================

#[derive(Debug, Clone)]
struct TheoMemory {
    id: Uuid,
    content: String,
}

async fn load_theo_memory(
    state: &AppState,
    user_id: Uuid,
) -> AppResult<Vec<TheoMemory>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, content
        FROM theo_memories
        WHERE user_id = $1
        ORDER BY updated_at DESC
        LIMIT 30
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, content)| TheoMemory { id, content })
        .collect())
}

async fn save_theo_memory(
    state: &AppState,
    user_id: Uuid,
    content: &str,
) -> AppResult<()> {
    let content = content.trim();

    if content.is_empty() {
        return Ok(());
    }

    sqlx::query(
        r#"
        INSERT INTO theo_memories (
            id,
            user_id,
            content,
            created_at,
            updated_at
        )
        VALUES (
            gen_random_uuid(),
            $1,
            $2,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        "#,
    )
    .bind(user_id)
    .bind(content)
    .execute(&state.db)
    .await?;

    Ok(())
}

fn build_memory_context(
    memories: &[TheoMemory],
) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let mut context =
        "MEMÓRIAS IMPORTANTES SOBRE ESTE USUÁRIO:\n\n"
            .to_string();

    for memory in memories.iter().take(20) {
        context.push_str("• ");
        context.push_str(&memory.content);
        context.push('\n');
    }

    context
}

fn extract_memory_request(
    message: &str,
) -> Option<String> {
    let normalized = normalize_text(message);

    let prefixes = [
        "lembre que ",
        "lembre-se que ",
        "lembrese que ",
        "memorize que ",
        "memoriza que ",
        "guarde que ",
        "guarda que ",
        "anote que ",
        "quero que voce lembre que ",
        "quero que o theo lembre que ",
    ];

    for prefix in prefixes {
        if normalized.starts_with(prefix) {
            let original_lower =
                message.to_lowercase();

            if let Some(position) =
                original_lower.find(prefix)
            {
                let memory = message
                    [position + prefix.len()..]
                    .trim();

                if !memory.is_empty() {
                    return Some(memory.to_string());
                }
            }
        }
    }

    None
}

fn detect_implicit_memory(
    message: &str,
) -> Option<String> {
    let normalized = normalize_text(message);

    let patterns = [
        (
            "tenho dificuldade em ",
            "O usuário tem dificuldade em ",
        ),
        (
            "tenho dificuldade com ",
            "O usuário tem dificuldade com ",
        ),
        (
            "tenho dificuldade para ",
            "O usuário tem dificuldade para ",
        ),
        (
            "quero focar em ",
            "O usuário quer focar em ",
        ),
        (
            "quero estudar mais ",
            "O usuário quer estudar mais ",
        ),
        (
            "quero priorizar ",
            "O usuário quer priorizar ",
        ),
        (
            "minha prova e ",
            "A prova do usuário é ",
        ),
        (
            "minha prova sera ",
            "A prova do usuário será ",
        ),
        (
            "estou estudando para ",
            "O usuário está estudando para ",
        ),
        (
            "estou me preparando para ",
            "O usuário está se preparando para ",
        ),
        (
            "meu objetivo e ",
            "O objetivo do usuário é ",
        ),
        (
            "meu objetivo é ",
            "O objetivo do usuário é ",
        ),
    ];

    for (pattern, description) in patterns {
        if normalized.starts_with(pattern) {
            let original_lower =
                message.to_lowercase();

            if let Some(position) =
                original_lower.find(pattern)
            {
                let value = message
                    [position + pattern.len()..]
                    .trim();

                if !value.is_empty() {
                    return Some(format!(
                        "{}{}",
                        description,
                        value
                    ));
                }
            }
        }
    }

    None
}

// ============================================================
// RECOMMENDATIONS
// ============================================================

async fn recommendations(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<serde_json::Value>> {
    let signals: Vec<(
        Uuid,
        String,
        i64,
        i64,
        i64,
        i64,
    )> = sqlx::query_as(
        r#"
        SELECT
            s.id,
            s.name,
            COALESCE(SUM(l.questions_total), 0) AS q_total,
            COALESCE(SUM(l.questions_correct), 0) AS q_correct,
            COALESCE(
                (
                    SELECT SUM(l2.minutes)
                    FROM study_logs l2
                    JOIN exam_topics t2
                        ON t2.id = l2.topic_id
                    WHERE t2.subject_id = s.id
                      AND l2.studied_on >=
                          date(CURRENT_TIMESTAMP, '-7 days')
                ),
                0
            ) AS minutes_7d,
            COUNT(DISTINCT t.id) AS topics_total
        FROM exam_subjects s
        JOIN exams e
            ON e.id = s.exam_id
        LEFT JOIN exam_topics t
            ON t.subject_id = s.id
        LEFT JOIN study_logs l
            ON l.topic_id = t.id
        WHERE e.user_id = $1
        GROUP BY s.id, s.name
        "#,
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    let mut out =
        Vec::with_capacity(signals.len());

    for (
        subject_id,
        subject_name,
        q_total,
        q_correct,
        minutes_7d,
        topics_total,
    ) in signals
    {
        let topics_studied: i64 =
            sqlx::query_scalar(
                r#"
                SELECT COUNT(DISTINCT t.id)
                FROM exam_topics t
                JOIN study_logs l
                    ON l.topic_id = t.id
                WHERE t.subject_id = $1
                "#,
            )
            .bind(subject_id)
            .fetch_one(&state.db)
            .await?;

        let accuracy = if q_total > 0 {
            q_correct as f64 / q_total as f64
        } else {
            0.0
        };

        let (action, reason) =
            if q_total >= 10 && accuracy >= 0.85 {
                (
                    "reduzir",
                    format!(
                        "{}% de acerto em {} questões — você já domina bem essa matéria. Dá para reduzir gradualmente o tempo, sem abandonar as revisões.",
                        (accuracy * 100.0).round(),
                        q_total
                    ),
                )
            } else if q_total >= 10 && accuracy < 0.60 {
                (
                    "aumentar",
                    format!(
                        "{}% de acerto em {} questões — essa matéria merece mais atenção antes de você avançar.",
                        (accuracy * 100.0).round(),
                        q_total
                    ),
                )
            } else if q_total < 5 {
                (
                    "manter",
                    "Ainda tenho poucas questões para avaliar essa matéria com segurança. Continue estudando e registrando seu desempenho."
                        .to_string(),
                )
            } else {
                (
                    "manter",
                    format!(
                        "{}% de acerto — seu desempenho está dentro de uma faixa razoável. Não vejo, por enquanto, um motivo forte para mudar o ritmo.",
                        (accuracy * 100.0).round()
                    ),
                )
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

    out.sort_by(|a, b| {
        a.accuracy
            .partial_cmp(&b.accuracy)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(Json(json!({
        "subjects": out
    })))
}

// ============================================================
// THEO CHAT
// ============================================================

async fn chat(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(payload): Json<ChatRequest>,
) -> AppResult<Json<ChatResponse>> {
    let message = payload.message.trim();

    if message.is_empty() {
        return Ok(Json(ChatResponse {
            message:
                "Digite uma pergunta para o Theo."
                    .to_string(),
            conversation_id: None,
            sources: vec![],
        }));
    }

    // ========================================================
    // CARREGA MEMÓRIA
    // ========================================================

    let memories =
        load_theo_memory(&state, user.id).await?;

    let _memory_context =
        build_memory_context(&memories);

    // ========================================================
    // SALVA MEMÓRIA EXPLÍCITA
    // ========================================================

    if let Some(memory) =
        extract_memory_request(message)
    {
        save_theo_memory(
            &state,
            user.id,
            &memory,
        )
        .await?;

        return Ok(Json(ChatResponse {
            message: format!(
                "Pode deixar. Vou guardar isso para lembrar nas próximas conversas: {}",
                memory
            ),
            conversation_id: None,
            sources: vec![],
        }));
    }

    // ========================================================
    // DETECTA MEMÓRIA IMPLÍCITA
    // ========================================================

    if let Some(memory) =
        detect_implicit_memory(message)
    {
        save_theo_memory(
            &state,
            user.id,
            &memory,
        )
        .await?;
    }

    // ========================================================
    // COMANDOS BÁSICOS
    // ========================================================

    if let Some(response) =
        handle_basic_command(message)
    {
        return Ok(Json(ChatResponse {
            message: response,
            conversation_id: None,
            sources: vec![],
        }));
    }

    // ========================================================
    // MATEMÁTICA
    // ========================================================

    if let Some(response) =
        handle_math_command(message)
    {
        return Ok(Json(ChatResponse {
            message: response,
            conversation_id: None,
            sources: vec![],
        }));
    }

    // ========================================================
    // LEGISLAÇÃO — PLANALTO
    // ========================================================

    if is_legal_question(message) {
        let result =
            handle_legal_question(message).await?;

        return Ok(Json(ChatResponse {
            message: result.message,
            conversation_id: None,
            sources: result.sources,
        }));
    }

    // ========================================================
    // LEARNING ENGINE
    // ========================================================

    let subjects =
        get_subject_signals(&state, user.id).await?;

    let total_questions: i64 =
        subjects
            .iter()
            .map(|s| s.questions_total)
            .sum();

    let total_correct: i64 =
        subjects
            .iter()
            .map(|s| s.questions_correct)
            .sum();

    let total_minutes: i64 =
        subjects
            .iter()
            .map(|s| s.minutes_7d)
            .sum();

    let accuracy =
        if total_questions > 0 {
            total_correct as f64
                / total_questions as f64
        } else {
            0.0
        };

    let weak_subjects:
        Vec<&SubjectChatSignal> =
        subjects
            .iter()
            .filter(|subject| {
                subject.questions_total >= 10
                    && subject.accuracy < 0.60
            })
            .collect();

    let strong_subjects:
        Vec<&SubjectChatSignal> =
        subjects
            .iter()
            .filter(|subject| {
                subject.questions_total >= 10
                    && subject.accuracy >= 0.85
            })
            .collect();

    let normalized =
        normalize_text(message);

    let response =
        if normalized.contains("ponto fraco")
            || normalized.contains("pontos fracos")
            || normalized.contains("fraca")
            || normalized.contains("fracas")
        {
            build_weak_subject_response(
                &weak_subjects,
            )
        } else if normalized.contains("preparacao")
            || normalized.contains("preparo")
            || normalized.contains("desempenho")
        {
            build_performance_response(
                accuracy,
                total_questions,
                total_minutes,
                &weak_subjects,
                &strong_subjects,
            )
        } else if normalized.contains("estrategia")
            || normalized.contains("plano")
            || normalized.contains("estudar")
        {
            build_strategy_response(
                &weak_subjects,
                &strong_subjects,
            )
        } else if normalized.contains("quanto estudei")
            || normalized.contains("quantas questoes")
            || normalized.contains("precisao")
            || normalized.contains("acerto")
        {
            build_performance_response(
                accuracy,
                total_questions,
                total_minutes,
                &weak_subjects,
                &strong_subjects,
            )
        } else if normalized.contains("materias fortes")
            || normalized.contains("materias forte")
            || normalized.contains("o que estou indo bem")
        {
            build_strong_subject_response(
                &strong_subjects,
            )
        } else if normalized.contains("me conhe")
            || normalized.contains("o que voce sabe sobre mim")
            || normalized.contains("o que voce lembra")
        {
            build_memory_response(&memories)
        } else {
            build_unknown_response(message)
        };

    let _conversation =
        payload.conversation;

    let _source_ids =
        payload.source_ids;

    Ok(Json(ChatResponse {
        message: response,
        conversation_id: None,
        sources: vec![],
    }))
}

// ============================================================
// COMANDOS BÁSICOS
// ============================================================

fn handle_basic_command(
    message: &str,
) -> Option<String> {
    let normalized =
        normalize_text(message);

    if is_any(
        &normalized,
        &[
            "oi",
            "ola",
            "oi theo",
            "ola theo",
            "e ai",
            "eai",
        ],
    ) {
        return Some(
            "Olá! 👋\n\nSou o Theo. Estou aqui para acompanhar sua preparação, não só para responder perguntas.\n\nPosso analisar seu desempenho, ajudar com os estudos, consultar legislação oficial, fazer cálculos e lembrar de coisas importantes que você me contar."
                .to_string(),
        );
    }

    if is_any(
        &normalized,
        &["bom dia", "bom dia theo"],
    ) {
        return Some(
            "Bom dia! 🫡\n\nVamos começar bem. Se quiser, posso olhar seu desempenho e dizer onde vale a pena colocar energia hoje."
                .to_string(),
        );
    }

    if is_any(
        &normalized,
        &[
            "boa tarde",
            "boa tarde theo",
        ],
    ) {
        return Some(
            "Boa tarde! 🫡\n\nComo está a preparação por aí? Se quiser, posso analisar seus dados ou simplesmente conversar sobre o que você precisa estudar."
                .to_string(),
        );
    }

    if is_any(
        &normalized,
        &[
            "boa noite",
            "boa noite theo",
        ],
    ) {
        return Some(
            "Boa noite! 🌙\n\nSe ainda estiver estudando, posso ajudar a decidir o que vale a pena revisar. Se já estiver encerrando, também posso resumir como foi sua preparação."
                .to_string(),
        );
    }

    if is_any(
        &normalized,
        &[
            "obrigado",
            "obrigada",
            "valeu",
            "brigado",
            "brigada",
        ],
    ) {
        return Some(
            "Disponha! 🫡\n\nEstou por aqui. Um pouco de constância todos os dias costuma valer mais do que tentar compensar tudo de uma vez."
                .to_string(),
        );
    }

    if is_any(
        &normalized,
        &[
            "quem e voce",
            "quem e o theo",
            "o que voce e",
        ],
    ) {
        return Some(
            "Eu sou o Theo, seu assistente de preparação.\n\nA ideia é ir além de simplesmente responder: posso acompanhar seus dados de estudo, identificar dificuldades, sugerir estratégias, consultar legislação oficial, resolver cálculos e guardar informações que você queira que eu lembre."
                .to_string(),
        );
    }

    if is_any(
        &normalized,
        &[
            "ajuda",
            "comandos",
            "o que voce pode fazer",
            "o que voce faz",
            "me ajude",
        ],
    ) {
        return Some(
            build_help_response(),
        );
    }

    None
}

fn build_help_response() -> String {
    "🫡 POSSO TE AJUDAR COM\n\n\
     CONVERSA\n\
     • oi\n\
     • bom dia\n\
     • quem é você\n\
     • o que você pode fazer\n\n\
     MEMÓRIA\n\
     • lembre que minha prova é em novembro\n\
     • tenho dificuldade em Direito Constitucional\n\
     • quero focar em Português\n\
     • meu objetivo é passar neste concurso\n\
     • o que você lembra sobre mim?\n\n\
     MATEMÁTICA\n\
     • calcule 10 + 20\n\
     • quanto é 50 - 17\n\
     • 20% de 500\n\
     • média de 10, 20, 30\n\
     • soma 10, 20, 30\n\
     • maior entre 10, 20, 50\n\
     • menor entre 10, 20, 50\n\
     • fatorial de 5\n\
     • raiz quadrada de 144\n\
     • 2 elevado a 8\n\
     • resto de 17 por 5\n\
     • módulo de -20\n\
     • seno de 30\n\
     • cosseno de 60\n\
     • tangente de 45\n\
     • arredonde 3.14159\n\n\
     PREPARAÇÃO\n\
     • quais são meus pontos fracos?\n\
     • como está minha preparação?\n\
     • qual meu desempenho?\n\
     • quais matérias estão fortes?\n\
     • o que devo estudar?\n\
     • monte uma estratégia\n\
     • quanto estudei?\n\
     • quantas questões fiz?\n\
     • qual minha precisão?\n\n\
     LEGISLAÇÃO\n\
     • o que diz a Constituição sobre liberdade de expressão?\n\
     • o que diz a Lei 14.133?\n\
     • explique o artigo 37 da Constituição\n\
     • o que diz a LGPD?\n\
     • o que diz a Lei 8.112?\n\n\
     Pode escrever normalmente também. Não precisa decorar comandos."
        .to_string()
}

// ============================================================
// MEMÓRIA — RESPOSTA
// ============================================================

fn build_memory_response(
    memories: &[TheoMemory],
) -> String {
    if memories.is_empty() {
        return "Ainda não guardei nenhuma informação pessoal sua.\n\nSe quiser, pode me dizer algo como: \"Lembre que tenho dificuldade em Direito Constitucional\"."
            .to_string();
    }

    let mut response =
        "Claro. Estas são as coisas que tenho guardadas sobre você:\n\n"
            .to_string();

    for memory in memories.iter().take(10) {
        response.push_str("• ");
        response.push_str(&memory.content);
        response.push('\n');
    }

    response.push_str(
        "\nSe alguma dessas informações mudar, é só me avisar.",
    );

    response
}

fn build_strong_subject_response(
    strong_subjects: &[&SubjectChatSignal],
) -> String {
    if strong_subjects.is_empty() {
        return "Ainda não tenho dados suficientes para apontar matérias realmente fortes. Conforme você fizer mais questões, consigo separar melhor o que está consolidado do que ainda precisa de atenção."
            .to_string();
    }

    let mut response =
        "Pelos seus dados, estas são as matérias em que você está indo melhor:\n\n"
            .to_string();

    for subject in strong_subjects.iter().take(5) {
        response.push_str(&format!(
            "• {} — {:.0}% de acerto em {} questões.\n",
            subject.subject_name,
            subject.accuracy * 100.0,
            subject.questions_total,
        ));
    }

    response.push_str(
        "\nEu manteria essas matérias em revisão, mas evitaria gastar nelas o mesmo tempo que você precisa investir nas áreas mais fracas.",
    );

    response
}

// ============================================================
// MATEMÁTICA
// ============================================================

fn handle_math_command(
    message: &str,
) -> Option<String> {
    let normalized =
        normalize_text(message);

    if let Some((percent, value)) =
        parse_percentage_of(&normalized)
    {
        let result =
            value * percent / 100.0;

        return Some(format!(
            "🧮 {}% de {} = {}",
            format_number(percent),
            format_number(value),
            format_number(result)
        ));
    }

    if let Some((value, percent)) =
        parse_percentage_change(
            &normalized,
            "aumente",
        )
    {
        let result =
            value * (1.0 + percent / 100.0);

        return Some(format!(
            "🧮 Aumentando {} em {}%:\n\nResultado = {}",
            format_number(value),
            format_number(percent),
            format_number(result)
        ));
    }

    if let Some((value, percent)) =
        parse_percentage_change(
            &normalized,
            "reduza",
        )
    {
        let result =
            value * (1.0 - percent / 100.0);

        return Some(format!(
            "🧮 Reduzindo {} em {}%:\n\nResultado = {}",
            format_number(value),
            format_number(percent),
            format_number(result)
        ));
    }

    if normalized.contains("media de")
        || normalized.starts_with("media ")
    {
        if let Some(numbers) =
            extract_numbers(&normalized)
        {
            if !numbers.is_empty() {
                let sum: f64 =
                    numbers.iter().sum();

                let result =
                    sum / numbers.len() as f64;

                return Some(format!(
                    "🧮 Média\n\n\
                     Valores: {}\n\
                     Soma: {}\n\
                     Quantidade: {}\n\
                     Resultado: {}",
                    format_numbers(&numbers),
                    format_number(sum),
                    numbers.len(),
                    format_number(result)
                ));
            }
        }
    }

    if normalized.contains("soma")
        || normalized.starts_with("some ")
    {
        if let Some(numbers) =
            extract_numbers(&normalized)
        {
            if !numbers.is_empty() {
                let result: f64 =
                    numbers.iter().sum();

                return Some(format!(
                    "🧮 Soma: {}\n\nResultado = {}",
                    format_numbers(&numbers),
                    format_number(result)
                ));
            }
        }
    }

    if normalized.contains("maior entre")
        || normalized.contains("maior de")
    {
        if let Some(numbers) =
            extract_numbers(&normalized)
        {
            if let Some(result) =
                numbers.iter().copied().reduce(f64::max)
            {
                return Some(format!(
                    "🧮 Maior valor: {}",
                    format_number(result)
                ));
            }
        }
    }

    if normalized.contains("menor entre")
        || normalized.contains("menor de")
    {
        if let Some(numbers) =
            extract_numbers(&normalized)
        {
            if let Some(result) =
                numbers.iter().copied().reduce(f64::min)
            {
                return Some(format!(
                    "🧮 Menor valor: {}",
                    format_number(result)
                ));
            }
        }
    }

    if normalized.contains("fatorial") {
        if let Some(number) =
            first_number(&normalized)
        {
            if number >= 0.0
                && number <= 170.0
                && number.fract() == 0.0
            {
                let n = number as u64;
                let result = factorial(n);

                return Some(format!(
                    "🧮 {}! = {}",
                    n,
                    result
                ));
            }

            return Some(
                "O fatorial precisa ser um número inteiro entre 0 e 170."
                    .to_string(),
            );
        }
    }

    if normalized.contains("raiz quadrada")
        || normalized.contains("raiz de")
    {
        if let Some(number) =
            first_number(&normalized)
        {
            if number >= 0.0 {
                let result = number.sqrt();

                return Some(format!(
                    "🧮 √{} = {}",
                    format_number(number),
                    format_number(result)
                ));
            }

            return Some(
                "Não existe raiz quadrada real de um número negativo."
                    .to_string(),
            );
        }
    }

    if normalized.contains("elevado") {
        if let Some(numbers) =
            extract_numbers(&normalized)
        {
            if numbers.len() >= 2 {
                let base = numbers[0];
                let exponent = numbers[1];
                let result =
                    base.powf(exponent);

                return Some(format!(
                    "🧮 {} elevado a {} = {}",
                    format_number(base),
                    format_number(exponent),
                    format_number(result)
                ));
            }
        }
    }

    if normalized.contains("modulo de")
        || normalized.contains("valor absoluto de")
    {
        if let Some(number) =
            first_number(&normalized)
        {
            return Some(format!(
                "🧮 |{}| = {}",
                format_number(number),
                format_number(number.abs())
            ));
        }
    }

    if normalized.contains("logaritmo")
        || normalized.contains("log de")
    {
        if let Some(number) =
            first_number(&normalized)
        {
            if number > 0.0 {
                return Some(format!(
                    "🧮 log₁₀({}) = {}",
                    format_number(number),
                    format_number(number.log10())
                ));
            }

            return Some(
                "O logaritmo real precisa de um número maior que zero."
                    .to_string(),
            );
        }
    }

    if normalized.contains("seno de") {
        if let Some(number) =
            first_number(&normalized)
        {
            let result =
                number.to_radians().sin();

            return Some(format!(
                "🧮 sen({}°) = {}",
                format_number(number),
                format_number(result)
            ));
        }
    }

    if normalized.contains("cosseno de") {
        if let Some(number) =
            first_number(&normalized)
        {
            let result =
                number.to_radians().cos();

            return Some(format!(
                "🧮 cos({}°) = {}",
                format_number(number),
                format_number(result)
            ));
        }
    }

    if normalized.contains("tangente de") {
        if let Some(number) =
            first_number(&normalized)
        {
            let result =
                number.to_radians().tan();

            return Some(format!(
                "🧮 tan({}°) = {}",
                format_number(number),
                format_number(result)
            ));
        }
    }

    if normalized.contains("arredonde")
        || normalized.contains("arredondar")
    {
        if let Some(number) =
            first_number(&normalized)
        {
            return Some(format!(
                "🧮 {} arredondado = {}",
                format_number(number),
                number.round()
            ));
        }
    }

    if normalized.contains("resto de")
        || normalized.contains("resto ")
    {
        if let Some(numbers) =
            extract_numbers(&normalized)
        {
            if numbers.len() >= 2 {
                let a = numbers[0];
                let b = numbers[1];

                if b != 0.0 {
                    return Some(format!(
                        "🧮 Resto de {} por {} = {}",
                        format_number(a),
                        format_number(b),
                        format_number(a % b)
                    ));
                }

                return Some(
                    "Não é possível calcular resto por zero."
                        .to_string(),
                );
            }
        }
    }

    if normalized.contains("calcule")
        || normalized.contains("quanto e")
        || normalized.contains("quanto é")
    {
        if let Some(result) =
            evaluate_simple_expression(&normalized)
        {
            return Some(format!(
                "🧮 Resultado: {}",
                format_number(result)
            ));
        }
    }

    if looks_like_math_expression(
        &normalized,
    ) {
        if let Some(result) =
            evaluate_simple_expression(&normalized)
        {
            return Some(format!(
                "🧮 {} = {}",
                message,
                format_number(result)
            ));
        }
    }

    None
}

// ============================================================
// PARSERS MATEMÁTICOS
// ============================================================

fn parse_percentage_of(
    text: &str,
) -> Option<(f64, f64)> {
    let percent_position =
        text.find('%')?;

    let before =
        &text[..percent_position];

    let percent =
        last_number(before)?;

    let after =
        &text[percent_position + 1..];

    let after = after
        .trim_start()
        .strip_prefix("de")
        .unwrap_or(after)
        .trim();

    let value =
        first_number(after)?;

    Some((percent, value))
}

fn parse_percentage_change(
    text: &str,
    command: &str,
) -> Option<(f64, f64)> {
    let text =
        text.strip_prefix(command)?.trim();

    let value =
        first_number(text)?;

    let percent_position =
        text.find('%')?;

    let before_percent =
        &text[..percent_position];

    let percent =
        last_number(before_percent)?;

    Some((value, percent))
}

fn extract_numbers(
    text: &str,
) -> Option<Vec<f64>> {
    let mut numbers =
        Vec::new();

    let chars: Vec<char> =
        text.chars().collect();

    let mut current =
        String::new();

    for (index, ch) in
        chars.iter().enumerate()
    {
        let previous =
            if index > 0 {
                Some(chars[index - 1])
            } else {
                None
            };

        let is_negative_sign =
            *ch == '-'
                && (
                    index == 0
                        || previous
                            .map(|c| {
                                !c.is_ascii_digit()
                            })
                            .unwrap_or(true)
                );

        let is_number =
            ch.is_ascii_digit()
                || *ch == '.'
                || *ch == ','
                || is_negative_sign;

        if is_number {
            current.push(*ch);
        } else if !current.is_empty() {
            if let Some(number) =
                parse_number(&current)
            {
                numbers.push(number);
            }

            current.clear();
        }
    }

    if !current.is_empty() {
        if let Some(number) =
            parse_number(&current)
        {
            numbers.push(number);
        }
    }

    if numbers.is_empty() {
        None
    } else {
        Some(numbers)
    }
}

fn first_number(
    text: &str,
) -> Option<f64> {
    extract_numbers(text)?
        .into_iter()
        .next()
}

fn last_number(
    text: &str,
) -> Option<f64> {
    extract_numbers(text)?
        .into_iter()
        .last()
}

fn parse_number(
    value: &str,
) -> Option<f64> {
    value
        .replace(',', ".")
        .parse::<f64>()
        .ok()
}

fn evaluate_simple_expression(
    text: &str,
) -> Option<f64> {
    let mut expression = text
        .replace("quanto e", "")
        .replace("quanto é", "")
        .replace("calcule", "")
        .replace("resultado de", "")
        .trim()
        .to_string();

    expression = expression
        .replace("vezes", "*")
        .replace("multiplicado por", "*")
        .replace("dividido por", "/")
        .replace("mais", "+")
        .replace("menos", "-");

    let operators = ['+', '-', '*', '/'];

    for operator in operators {
        if let Some(position) =
            find_binary_operator(
                &expression,
                operator,
            )
        {
            let left =
                expression[..position].trim();

            let right =
                expression[position + 1..]
                    .trim();

            let a =
                last_number(left)?;

            let b =
                first_number(right)?;

            return match operator {
                '+' => Some(a + b),
                '-' => Some(a - b),
                '*' => Some(a * b),
                '/' => {
                    if b == 0.0 {
                        None
                    } else {
                        Some(a / b)
                    }
                }
                _ => None,
            };
        }
    }

    None
}

fn find_binary_operator(
    expression: &str,
    operator: char,
) -> Option<usize> {
    let chars: Vec<char> =
        expression.chars().collect();

    for (index, ch) in
        chars.iter().enumerate()
    {
        if *ch != operator {
            continue;
        }

        if *ch == '-' {
            if index == 0 {
                continue;
            }

            let previous =
                chars[index - 1];

            if matches!(
                previous,
                '+' | '-' | '*' | '/'
            ) {
                continue;
            }
        }

        return Some(
            chars[..index]
                .iter()
                .map(|c| c.len_utf8())
                .sum(),
        );
    }

    None
}

fn looks_like_math_expression(
    text: &str,
) -> bool {
    let has_operator =
        text.contains('+')
            || text.contains('*')
            || text.contains('/');

    let has_minus =
        text.contains('-');

    let has_number =
        text.chars()
            .any(|c| c.is_ascii_digit());

    has_number
        && (has_operator || has_minus)
}

fn factorial(n: u64) -> u128 {
    let mut result: u128 = 1;

    for i in 2..=n {
        result *= i as u128;
    }

    result
}

// ============================================================
// FORMATAÇÃO
// ============================================================

fn format_number(
    value: f64,
) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{:.6}", value)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .replace('.', ",")
    }
}

fn format_numbers(
    numbers: &[f64],
) -> String {
    numbers
        .iter()
        .map(|n| format_number(*n))
        .collect::<Vec<_>>()
        .join(", ")
}

// ============================================================
// NORMALIZAÇÃO
// ============================================================

fn normalize_text(
    text: &str,
) -> String {
    text.to_lowercase()
        .replace('á', "a")
        .replace('à', "a")
        .replace('ã', "a")
        .replace('â', "a")
        .replace('ä', "a")
        .replace('é', "e")
        .replace('è', "e")
        .replace('ê', "e")
        .replace('ë', "e")
        .replace('í', "i")
        .replace('ì', "i")
        .replace('î', "i")
        .replace('ï', "i")
        .replace('ó', "o")
        .replace('ò', "o")
        .replace('õ', "o")
        .replace('ô', "o")
        .replace('ö', "o")
        .replace('ú', "u")
        .replace('ù', "u")
        .replace('û', "u")
        .replace('ü', "u")
        .replace('ç', "c")
}

fn is_any(
    text: &str,
    values: &[&str],
) -> bool {
    values
        .iter()
        .any(|value| text == *value)
}

// ============================================================
// DADOS DO THEO
// ============================================================

#[derive(Debug)]
struct SubjectChatSignal {
    subject_name: String,
    accuracy: f64,
    questions_total: i64,
    questions_correct: i64,
    minutes_7d: i64,
}

async fn get_subject_signals(
    state: &AppState,
    user_id: Uuid,
) -> AppResult<Vec<SubjectChatSignal>> {
    let rows: Vec<(
        String,
        i64,
        i64,
        i64,
    )> = sqlx::query_as(
        r#"
        SELECT
            s.name,
            COALESCE(
                SUM(l.questions_total),
                0
            ) AS questions_total,
            COALESCE(
                SUM(l.questions_correct),
                0
            ) AS questions_correct,
            COALESCE(
                (
                    SELECT SUM(l2.minutes)
                    FROM study_logs l2
                    JOIN exam_topics t2
                        ON t2.id = l2.topic_id
                    WHERE t2.subject_id = s.id
                      AND l2.studied_on >=
                          date(CURRENT_TIMESTAMP, '-7 days')
                ),
                0
            ) AS minutes_7d
        FROM exam_subjects s
        JOIN exams e
            ON e.id = s.exam_id
        LEFT JOIN exam_topics t
            ON t.subject_id = s.id
        LEFT JOIN study_logs l
            ON l.topic_id = t.id
        WHERE e.user_id = $1
        GROUP BY s.id, s.name
        ORDER BY s.name
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                subject_name,
                questions_total,
                questions_correct,
                minutes_7d,
            )| {
                let accuracy =
                    if questions_total > 0 {
                        questions_correct as f64
                            / questions_total as f64
                    } else {
                        0.0
                    };

                SubjectChatSignal {
                    subject_name,
                    accuracy,
                    questions_total,
                    questions_correct,
                    minutes_7d,
                }
            },
        )
        .collect())
}

// ============================================================
// LEGISLAÇÃO — PLANALTO
// ============================================================

struct LegalResponse {
    message: String,
    sources: Vec<ChatSource>,
}

fn is_legal_question(
    message: &str,
) -> bool {
    let normalized =
        normalize_text(message);

    let legal_terms = [
        "constituicao",
        "constituicao federal",
        "codigo civil",
        "codigo penal",
        "codigo de processo civil",
        "codigo de processo penal",
        "codigo de defesa do consumidor",
        "lei ",
        "decreto",
        "artigo ",
        "art. ",
        "paragrafo",
        "inciso",
        "licitacao",
        "licitacoes",
        "contrato administrativo",
        "contratos administrativos",
        "servidor publico",
        "servidores publicos",
        "lgpd",
        "dados pessoais",
        "acesso a informacao",
        "responsabilidade fiscal",
        "eca",
        "estatuto do idoso",
        "direito constitucional",
        "direito administrativo",
        "direito penal",
        "direito civil",
    ];

    legal_terms
        .iter()
        .any(|term| normalized.contains(term))
}

async fn handle_legal_question(
    message: &str,
) -> AppResult<LegalResponse> {
    let planalto =
        PlanaltoService::new();

    let results =
        planalto.search(message).await?;

    if results.is_empty() {
        return Ok(LegalResponse {
            message: format!(
                "⚖️ Não consegui encontrar uma legislação específica no Portal do Planalto para essa pergunta.\n\nPergunta: \"{}\"\n\nSe puder, informe o número da lei, código ou artigo. Assim consigo fazer uma busca mais precisa.",
                message
            ),
            sources: vec![],
        });
    }

    let mut response =
        String::new();

    response.push_str(
        "⚖️ Encontrei estas referências oficiais no Portal do Planalto.\n\n",
    );

    for result in results.iter().take(3) {
        response.push_str(
            &format!(
                "📚 {}\n\n",
                result.title
            ),
        );

        let excerpt =
            build_legal_excerpt(
                &result.content,
                message,
            );

        if !excerpt.is_empty() {
            response.push_str(&excerpt);
            response.push_str("\n\n");
        }
    }

    response.push_str(
        "📌 A resposta foi baseada nas fontes oficiais encontradas. Se você me passar um artigo específico, consigo direcionar melhor a explicação.",
    );

    let sources = results
        .into_iter()
        .take(3)
        .map(|result| ChatSource {
            title: result.title,
            url: result.url,
            source: result.source,
        })
        .collect();

    Ok(LegalResponse {
        message: response,
        sources,
    })
}

fn build_legal_excerpt(
    content: &str,
    question: &str,
) -> String {
    let normalized_question =
        normalize_text(question);

    let keywords: Vec<&str> =
        normalized_question
            .split_whitespace()
            .filter(|word| word.len() >= 5)
            .filter(|word| {
                !matches!(
                    *word,
                    "sobre"
                        | "quais"
                        | "qual"
                        | "como"
                        | "quando"
                        | "onde"
                        | "pode"
                        | "posso"
                        | "diz"
                        | "dizer"
                        | "explique"
                        | "explica"
                        | "direito"
                        | "legal"
                )
            })
            .take(10)
            .collect();

    let sentences: Vec<&str> =
        content
            .split(['.', ';'])
            .map(str::trim)
            .filter(|sentence| {
                sentence.len() > 40
            })
            .collect();

    let mut relevant:
        Vec<(usize, &str)> =
        Vec::new();

    for sentence in sentences {
        let normalized_sentence =
            normalize_text(sentence);

        let score = keywords
            .iter()
            .filter(|keyword| {
                normalized_sentence
                    .contains(**keyword)
            })
            .count();

        if score > 0 {
            relevant.push((
                score,
                sentence,
            ));
        }
    }

    relevant.sort_by(|a, b| {
        b.0.cmp(&a.0)
    });

    let excerpts: Vec<&str> =
        relevant
            .into_iter()
            .take(5)
            .map(|(_, sentence)| sentence)
            .collect();

    if excerpts.is_empty() {
        return content
            .split_whitespace()
            .take(120)
            .collect::<Vec<_>>()
            .join(" ");
    }

    excerpts.join(". ")
}

// ============================================================
// RESPOSTAS
// ============================================================

fn build_unknown_response(
    message: &str,
) -> String {
    format!(
        "Entendi o que você perguntou: \"{}\".\n\nAinda não tenho uma resposta específica para isso dentro das funções que estão conectadas a mim.\n\nMas posso te ajudar com seu desempenho, pontos fracos, estratégia de estudos, matemática, legislação oficial e também lembrar de informações importantes que você me contar.\n\nSe quiser, tente algo como:\n• Quais são meus pontos fracos?\n• Como está minha preparação?\n• O que devo estudar agora?\n• Calcule 25% de 800\n• O que diz a Lei 14.133?\n• O que você lembra sobre mim?",
        message
    )
}

fn build_weak_subject_response(
    weak_subjects: &[&SubjectChatSignal],
) -> String {
    if weak_subjects.is_empty() {
        return "Por enquanto, não encontrei uma matéria que eu consiga chamar de ponto fraco com segurança.\n\nPara fazer essa avaliação, preciso de pelo menos 10 questões registradas em uma matéria.\n\nContinue estudando e fazendo questões. Conforme os dados aumentarem, consigo ser mais preciso."
            .to_string();
    }

    let mut response =
        "Olhei seus dados com atenção. Estas são as matérias que mais estão pedindo cuidado agora:\n\n"
            .to_string();

    for subject in weak_subjects.iter().take(5) {
        response.push_str(&format!(
            "• {} — {:.0}% de acerto em {} questões.\n",
            subject.subject_name,
            subject.accuracy * 100.0,
            subject.questions_total,
        ));
    }

    response.push_str(
        "\nEu não abandonaria as outras matérias, mas colocaria um pouco mais de energia nessas áreas até o desempenho começar a reagir.",
    );

    response
}

fn build_performance_response(
    accuracy: f64,
    total_questions: i64,
    total_minutes: i64,
    weak_subjects: &[&SubjectChatSignal],
    strong_subjects: &[&SubjectChatSignal],
) -> String {
    let mut response =
        format!(
            "Dei uma olhada no que você registrou até agora.\n\nSeu aproveitamento geral está em {:.0}% em {} questões.\nNos últimos 7 dias, você registrou {} minutos de estudo.\n",
            accuracy * 100.0,
            total_questions,
            total_minutes,
        );

    if !weak_subjects.is_empty() {
        response.push_str(
            "\n⚠️ Onde eu teria mais cuidado:\n",
        );

        for subject in
            weak_subjects.iter().take(3)
        {
            response.push_str(&format!(
                "• {} — {:.0}% de acerto\n",
                subject.subject_name,
                subject.accuracy * 100.0,
            ));
        }
    }

    if !strong_subjects.is_empty() {
        response.push_str(
            "\n✓ Onde você já demonstra bom domínio:\n",
        );

        for subject in
            strong_subjects.iter().take(3)
        {
            response.push_str(&format!(
                "• {} — {:.0}% de acerto\n",
                subject.subject_name,
                subject.accuracy * 100.0,
            ));
        }
    }

    response.push_str(
        "\nMinha sugestão é não olhar só para o percentual. O mais importante é usar esses sinais para decidir onde seu próximo bloco de estudo vai render mais.",
    );

    response
}

fn build_strategy_response(
    weak_subjects: &[&SubjectChatSignal],
    strong_subjects: &[&SubjectChatSignal],
) -> String {
    let mut response =
        "Eu montaria sua estratégia pensando em prioridade, e não em tentar estudar tudo com a mesma intensidade.\n\n"
            .to_string();

    if !weak_subjects.is_empty() {
        response.push_str(
            "1. Dê mais espaço às matérias com menor desempenho.\n",
        );

        for subject in
            weak_subjects.iter().take(3)
        {
            response.push_str(&format!(
                "   • {} — {:.0}% de acerto\n",
                subject.subject_name,
                subject.accuracy * 100.0,
            ));
        }

        response.push('\n');
    }

    if !strong_subjects.is_empty() {
        response.push_str(
            "2. Nas matérias que já estão fortes, mantenha contato por meio de revisões e questões.\n",
        );

        for subject in
            strong_subjects.iter().take(3)
        {
            response.push_str(&format!(
                "   • {} — {:.0}% de acerto\n",
                subject.subject_name,
                subject.accuracy * 100.0,
            ));
        }

        response.push('\n');
    }

    response.push_str(
        "3. Continue registrando seus estudos e questões. Quanto melhor for o histórico, mais confiáveis ficam minhas sugestões.",
    );

    response
}