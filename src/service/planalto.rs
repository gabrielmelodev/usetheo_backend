use crate::error::AppResult;

use reqwest::Client;
use scraper::{Html, Selector};
use serde::Serialize;
use std::collections::HashSet;
use std::time::Duration;

const PLANALTO_BASE: &str = "https://www.planalto.gov.br";

const MAX_CONTENT_CHARS: usize = 120_000;
const MAX_EXCERPT_CHARS: usize = 6_000;

/// Fonte oficial encontrada.
#[derive(Debug, Clone, Serialize)]
pub struct PlanaltoSource {
    pub title: String,
    pub url: String,
    pub content: String,
}

/// Resultado jurídico recuperado.
#[derive(Debug, Clone, Serialize)]
pub struct PlanaltoSearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub source: String,

    /// Pontuação de relevância da fonte para a pergunta.
    pub score: u32,

    /// Artigo identificado na pergunta, quando houver.
    pub article: Option<String>,

    /// Trecho mais relevante encontrado.
    pub excerpt: String,
}

/// Serviço de consulta da legislação oficial.
#[derive(Clone)]
pub struct PlanaltoService {
    client: Client,
}

impl PlanaltoService {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(
                "Theo/1.0 (assistente educacional; consulta de legislacao oficial)",
            )
            .timeout(Duration::from_secs(20))
            .build()
            .expect("falha ao criar HTTP client do Planalto");

        Self { client }
    }

    // ============================================================
    // DOWNLOAD
    // ============================================================

    /// Baixa uma página oficial e transforma o HTML em texto limpo.
    pub async fn fetch_url(
        &self,
        url: &str,
    ) -> Result<PlanaltoSource, reqwest::Error> {
        let response = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?;

        let html = response.text().await?;

        let document = Html::parse_document(&html);

        let content = extract_clean_body(&document);

        let title = extract_title(&document)
            .unwrap_or_else(|| "Legislação — Portal do Planalto".to_string());

        Ok(PlanaltoSource {
            title,
            url: url.to_string(),
            content,
        })
    }

    // ============================================================
    // BUSCA INTELIGENTE
    // ============================================================

    /// Analisa a pergunta e recupera as normas mais relevantes.
    pub async fn search(
        &self,
        query: &str,
    ) -> AppResult<Vec<PlanaltoSearchResult>> {
        let normalized = normalize_text(query);

        if normalized.trim().is_empty() {
            return Ok(Vec::new());
        }

        let article = extract_article(query);

        let keywords = extract_keywords(&normalized);

        let candidates = legislation_candidates(&normalized);

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        for candidate in candidates {
            match self.fetch_url(&candidate.url).await {
                Ok(source) => {
                    let score = calculate_score(
                        &normalized,
                        &keywords,
                        &candidate,
                        &source.content,
                    );

                    let excerpt = build_relevant_excerpt(
                        &source.content,
                        &keywords,
                        article.as_deref(),
                    );

                    results.push(PlanaltoSearchResult {
    title: if source.title.is_empty() {
        candidate.title.to_string()
    } else {
        source.title.clone()
    },
    url: source.url,
    content: truncate_text(
        &source.content,
        MAX_CONTENT_CHARS,
    ),
    source: "Planalto".to_string(),
    score,
    article: article.clone(),
    excerpt,
});
                }

                Err(error) => {
                    tracing::warn!(
                        url = %candidate.url,
                        error = ?error,
                        "Falha ao consultar legislação do Planalto"
                    );
                }
            }
        }

        // Mais relevante primeiro.
        results.sort_by(|a, b| b.score.cmp(&a.score));

        // Mantém apenas resultados realmente úteis.
        results.retain(|result| result.score > 0);

        // Evita duplicações.
        let mut seen = HashSet::new();

        results.retain(|result| seen.insert(result.url.clone()));

        // Não sobrecarregar o chat com dezenas de leis.
        results.truncate(5);

        Ok(results)
    }
}

// ================================================================
// CANDIDATOS JURÍDICOS
// ================================================================

#[derive(Debug, Clone)]
struct LegislationCandidate {
    title: &'static str,
    url: String,
    aliases: &'static [&'static str],
    topics: &'static [&'static str],
}

fn legislation_candidates(
    query: &str,
) -> Vec<LegislationCandidate> {
    let mut candidates = Vec::new();

    // Constituição
    if contains_any(
        query,
        &[
            "constituicao",
            "constituicao federal",
            "cf 88",
            "cf88",
            "carta magna",
            "direitos fundamentais",
            "liberdade de expressao",
            "principios fundamentais",
            "administracao publica",
            "servidor publico",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Constituição Federal",
            url: format!(
                "{}/ccivil_03/constituicao/constituicaocompilado.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "constituicao",
                "constituicao federal",
                "cf",
                "carta magna",
            ],
            topics: &[
                "direitos fundamentais",
                "liberdade",
                "igualdade",
                "administracao publica",
                "servidor publico",
                "politica",
                "nacionalidade",
                "poderes",
            ],
        });
    }

    // Código Civil
    if contains_any(
        query,
        &[
            "codigo civil",
            "direito civil",
            "obrigacao",
            "contrato civil",
            "responsabilidade civil",
            "indenizacao",
            "propriedade",
            "heranca",
            "sucessao",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Código Civil",
            url: format!(
                "{}/ccivil_03/leis/2002/l10406compilada.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "codigo civil",
                "direito civil",
            ],
            topics: &[
                "contrato",
                "obrigacao",
                "responsabilidade",
                "indenizacao",
                "propriedade",
                "familia",
                "sucessao",
            ],
        });
    }

    // Código Penal
    if contains_any(
        query,
        &[
            "codigo penal",
            "direito penal",
            "crime",
            "crimes",
            "pena",
            "punibilidade",
            "homicidio",
            "furto",
            "roubo",
            "estelionato",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Código Penal",
            url: format!(
                "{}/ccivil_03/decreto-lei/del2848compilado.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "codigo penal",
                "direito penal",
            ],
            topics: &[
                "crime",
                "pena",
                "homicidio",
                "furto",
                "roubo",
                "estelionato",
                "culpa",
                "dolo",
            ],
        });
    }

    // CPC
    if contains_any(
        query,
        &[
            "codigo de processo civil",
            "cpc",
            "processo civil",
            "acao civil",
            "peticao",
            "recurso",
            "sentenca",
            "tutela",
            "execucao civil",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Código de Processo Civil",
            url: format!(
                "{}/ccivil_03/_ato2015-2018/2015/lei/l13105.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "codigo de processo civil",
                "cpc",
                "processo civil",
            ],
            topics: &[
                "processo",
                "acao",
                "recurso",
                "sentenca",
                "tutela",
                "execucao",
                "peticao",
            ],
        });
    }

    // CPP
    if contains_any(
        query,
        &[
            "codigo de processo penal",
            "cpp",
            "processo penal",
            "inquerito",
            "prisao",
            "flagrante",
            "investigacao criminal",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Código de Processo Penal",
            url: format!(
                "{}/ccivil_03/decreto-lei/del3689compilado.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "codigo de processo penal",
                "cpp",
                "processo penal",
            ],
            topics: &[
                "inquerito",
                "prisao",
                "flagrante",
                "investigacao",
                "prova",
                "processo penal",
            ],
        });
    }

    // CDC
    if contains_any(
        query,
        &[
            "codigo de defesa do consumidor",
            "cdc",
            "defesa do consumidor",
            "direito do consumidor",
            "consumidor",
            "fornecedor",
            "produto",
            "servico",
            "propaganda",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Código de Defesa do Consumidor",
            url: format!(
                "{}/ccivil_03/leis/l8078compilado.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "codigo de defesa do consumidor",
                "cdc",
                "consumidor",
            ],
            topics: &[
                "consumidor",
                "fornecedor",
                "produto",
                "servico",
                "propaganda",
                "contrato",
                "dano",
            ],
        });
    }

    // Lei 8.112
    if contains_any(
        query,
        &[
            "lei 8112",
            "lei 8.112",
            "8112",
            "servidor publico federal",
            "servidor federal",
            "regime juridico unico",
            "rju",
            "funcionario publico federal",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Lei nº 8.112/1990",
            url: format!(
                "{}/ccivil_03/leis/l8112cons.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "lei 8112",
                "lei 8.112",
                "8112",
            ],
            topics: &[
                "servidor",
                "servidor publico",
                "cargo",
                "provimento",
                "vacancia",
                "remuneracao",
                "licenca",
                "ferias",
                "aposentadoria",
                "penalidade",
            ],
        });
    }

    // Lei 14.133
    if contains_any(
        query,
        &[
            "lei 14133",
            "lei 14.133",
            "14133",
            "licitacao",
            "licitacoes",
            "contratacao publica",
            "contratos administrativos",
            "pregao",
            "dispensa de licitacao",
            "inexigibilidade",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Lei nº 14.133/2021",
            url: format!(
                "{}/ccivil_03/_ato2019-2022/2021/lei/l14133.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "lei 14133",
                "lei 14.133",
                "14133",
                "licitacao",
            ],
            topics: &[
                "licitacao",
                "contrato",
                "pregao",
                "dispensa",
                "inexigibilidade",
                "contratacao",
                "administracao publica",
            ],
        });
    }

    // Lei 9.784
    if contains_any(
        query,
        &[
            "lei 9784",
            "lei 9.784",
            "9784",
            "processo administrativo federal",
            "processo administrativo",
            "ato administrativo",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Lei nº 9.784/1999",
            url: format!(
                "{}/ccivil_03/leis/l9784.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "lei 9784",
                "lei 9.784",
                "9784",
            ],
            topics: &[
                "processo administrativo",
                "ato administrativo",
                "administracao",
                "servidor",
                "recurso",
                "prazo",
            ],
        });
    }

    // LRF
    if contains_any(
        query,
        &[
            "lei de responsabilidade fiscal",
            "lrf",
            "lc 101",
            "lei complementar 101",
            "responsabilidade fiscal",
            "gestao fiscal",
            "despesa com pessoal",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Lei Complementar nº 101/2000",
            url: format!(
                "{}/ccivil_03/leis/lcp/lcp101.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "lei de responsabilidade fiscal",
                "lrf",
                "lc 101",
            ],
            topics: &[
                "responsabilidade fiscal",
                "despesa",
                "receita",
                "divida",
                "pessoal",
                "orcamento",
                "gestao fiscal",
            ],
        });
    }

    // LAI
    if contains_any(
        query,
        &[
            "lei de acesso a informacao",
            "lai",
            "lei 12527",
            "lei 12.527",
            "acesso a informacao",
            "informacao publica",
            "transparencia",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Lei nº 12.527/2011",
            url: format!(
                "{}/ccivil_03/_ato2011-2014/2011/lei/l12527.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "lei 12527",
                "lei 12.527",
                "lai",
            ],
            topics: &[
                "informacao",
                "transparencia",
                "acesso",
                "sigilo",
                "publicidade",
            ],
        });
    }

    // LGPD
    if contains_any(
        query,
        &[
            "lgpd",
            "lei geral de protecao de dados",
            "lei 13709",
            "lei 13.709",
            "protecao de dados",
            "dados pessoais",
            "tratamento de dados",
            "controlador",
            "operador",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Lei nº 13.709/2018 — LGPD",
            url: format!(
                "{}/ccivil_03/_ato2015-2018/2018/lei/l13709.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "lgpd",
                "lei 13709",
                "lei 13.709",
            ],
            topics: &[
                "dados",
                "dados pessoais",
                "tratamento",
                "controlador",
                "operador",
                "privacidade",
                "consentimento",
            ],
        });
    }

    // ECA
    if contains_any(
        query,
        &[
            "eca",
            "estatuto da crianca",
            "lei 8069",
            "lei 8.069",
            "crianca e adolescente",
            "adolescente",
            "menor",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Estatuto da Criança e do Adolescente",
            url: format!(
                "{}/ccivil_03/leis/l8069.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "eca",
                "lei 8069",
                "estatuto da crianca",
            ],
            topics: &[
                "crianca",
                "adolescente",
                "familia",
                "educacao",
                "protecao",
                "ato infracional",
            ],
        });
    }

    // Estatuto da Pessoa Idosa
    if contains_any(
        query,
        &[
            "estatuto do idoso",
            "estatuto da pessoa idosa",
            "lei 10741",
            "lei 10.741",
            "direitos do idoso",
            "pessoa idosa",
        ],
    ) {
        candidates.push(LegislationCandidate {
            title: "Estatuto da Pessoa Idosa",
            url: format!(
                "{}/ccivil_03/leis/2003/l10.741.htm",
                PLANALTO_BASE
            ),
            aliases: &[
                "estatuto do idoso",
                "lei 10741",
                "lei 10.741",
            ],
            topics: &[
                "idoso",
                "pessoa idosa",
                "saude",
                "transporte",
                "familia",
                "protecao",
            ],
        });
    }

    candidates
}

// ================================================================
// PONTUAÇÃO
// ================================================================

fn calculate_score(
    query: &str,
    keywords: &[String],
    candidate: &LegislationCandidate,
    content: &str,
) -> u32 {
    let mut score = 0;

    // Nome/alias explícito da norma.
    for alias in candidate.aliases {
        if query.contains(alias) {
            score += 30;
        }
    }

    // Assunto jurídico.
    for topic in candidate.topics {
        if query.contains(topic) {
            score += 8;
        }
    }

    // Palavras importantes da pergunta encontradas no documento.
    let normalized_content = normalize_text(content);

    for keyword in keywords {
        if normalized_content.contains(keyword) {
            score += 2;
        }
    }

    // Se encontrou artigo, aumenta a relevância.
    if extract_article(query).is_some() {
        score += 10;
    }

    score
}

// ================================================================
// ARTIGO
// ================================================================

fn extract_article(text: &str) -> Option<String> {
    let normalized = normalize_text(text);

    let words: Vec<&str> = normalized.split_whitespace().collect();

    for i in 0..words.len() {
        let word = words[i];

        if word == "artigo" || word == "art." || word == "art" {
            if let Some(next) = words.get(i + 1) {
                let number = next
                    .trim_matches(|c: char| {
                        !c.is_ascii_digit()
                    });

                if !number.is_empty() {
                    return Some(format!("Art. {}", number));
                }
            }
        }
    }

    None
}

// ================================================================
// PALAVRAS-CHAVE
// ================================================================

fn extract_keywords(query: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "sobre",
        "quais",
        "qual",
        "como",
        "quando",
        "onde",
        "porque",
        "por",
        "que",
        "para",
        "com",
        "uma",
        "um",
        "uns",
        "umas",
        "dos",
        "das",
        "do",
        "da",
        "de",
        "na",
        "no",
        "nas",
        "nos",
        "e",
        "ou",
        "a",
        "o",
        "as",
        "os",
        "me",
        "se",
        "pode",
        "posso",
        "diz",
        "dizer",
        "explique",
        "explica",
        "explique-me",
        "fale",
        "falar",
        "lei",
        "artigo",
        "art",
        "direito",
    ];

    let mut keywords = Vec::new();

    for word in query.split_whitespace() {
        let cleaned = word
            .trim_matches(|c: char| !c.is_alphanumeric());

        if cleaned.len() < 4 {
            continue;
        }

        if STOPWORDS.contains(&cleaned) {
            continue;
        }

        if !keywords.contains(&cleaned.to_string()) {
            keywords.push(cleaned.to_string());
        }
    }

    keywords.truncate(15);

    keywords
}

// ================================================================
// EXCERTO INTELIGENTE
// ================================================================

fn build_relevant_excerpt(
    content: &str,
    keywords: &[String],
    article: Option<&str>,
) -> String {
    let normalized_content = normalize_text(content);

    // Primeiro tenta localizar o artigo solicitado.
    if let Some(article) = article {
        let article_normalized = normalize_text(article);

        if let Some(position) =
            normalized_content.find(&article_normalized)
        {
            return extract_window(
                content,
                position,
                MAX_EXCERPT_CHARS,
            );
        }
    }

    // Depois procura o trecho com maior concentração
    // de palavras da pergunta.
    let sentences = split_into_sentences(content);

    let mut scored: Vec<(u32, &str)> = sentences
        .iter()
        .map(|sentence| {
            let normalized = normalize_text(sentence);

            let score = keywords
                .iter()
                .filter(|keyword| normalized.contains(keyword.as_str()))
                .count() as u32;

            (score, *sentence)
        })
        .filter(|(score, sentence)| {
            *score > 0 && sentence.len() > 30
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));

    let mut excerpt = String::new();

    for (_, sentence) in scored.into_iter().take(8) {
        if excerpt.len() + sentence.len() > MAX_EXCERPT_CHARS {
            break;
        }

        if !excerpt.is_empty() {
            excerpt.push_str(" ");
        }

        excerpt.push_str(sentence);
    }

    if excerpt.is_empty() {
        return truncate_text(content, 2_000);
    }

    excerpt
}

// ================================================================
// HTML
// ================================================================

fn extract_clean_body(document: &Html) -> String {
    let selector = Selector::parse(
        "body",
    )
    .expect("seletor HTML body invalido");

    let mut pieces = Vec::new();

    for body in document.select(&selector) {
        for element in body.select(
            &Selector::parse(
                "script, style, noscript, nav, header, footer",
            )
            .expect("seletor HTML invalido"),
        ) {
            let _ = element;
        }

        for text in body.text() {
            let text = text.trim();

            if text.is_empty() {
                continue;
            }

            pieces.push(text.to_string());
        }
    }

    clean_text(
        &pieces.join(" "),
    )
}

// ================================================================
// TÍTULO
// ================================================================

fn extract_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;

    document
        .select(&selector)
        .next()
        .map(|element| {
            element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        })
        .filter(|title| !title.is_empty())
}

// ================================================================
// FRASES
// ================================================================

fn split_into_sentences(text: &str) -> Vec<&str> {
    text.split(|c| {
        matches!(c, '.' | ';' | ':' | '!' | '?')
    })
    .map(str::trim)
    .filter(|sentence| !sentence.is_empty())
    .collect()
}

// ================================================================
// JANELA DE TEXTO
// ================================================================

fn extract_window(
    content: &str,
    position: usize,
    max_chars: usize,
) -> String {
    let start = position.saturating_sub(1_500);

    let end = (position + max_chars)
        .min(content.len());

    content[start..end].to_string()
}

// ================================================================
// LIMPEZA
// ================================================================

fn clean_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ================================================================
// LIMITE
// ================================================================

fn truncate_text(
    text: &str,
    max_chars: usize,
) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    let mut end = max_chars;

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}…", &text[..end])
}

// ================================================================
// NORMALIZAÇÃO
// ================================================================

fn normalize_text(text: &str) -> String {
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

// ================================================================
// MATCH
// ================================================================

fn contains_any(
    text: &str,
    terms: &[&str],
) -> bool {
    terms.iter().any(|term| text.contains(term))
}