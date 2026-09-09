use crate::config::Config;
use serde_json::json;

/// Envia e-mails transacionais usando a API do Resend.
///
/// Se RESEND_API_KEY não estiver configurada, o e-mail é apenas
/// exibido no console. Isso facilita o desenvolvimento local.
pub async fn send_mail(
    config: &Config,
    to: &str,
    subject: &str,
    body: &str,
) -> anyhow::Result<()> {
    if !config.email_enabled {
        tracing::debug!("[email] desativado por EMAIL_ENABLED=false; destinatário={}", to);
        return Ok(());
    }

    // ============================================================
    // MODO DESENVOLVIMENTO
    // ============================================================

    let Some(api_key) = &config.resend_api_key else {
        tracing::info!(
            "[dev-mailer] para={} assunto={}\n{}",
            to,
            subject,
            body
        );

        return Ok(());
    };

    // ============================================================
    // LOG
    // ============================================================

    tracing::info!(
        "[resend] enviando e-mail para={} assunto={}",
        to,
        subject
    );

    // ============================================================
    // CLIENT HTTP
    // ============================================================

    let client = reqwest::Client::new();

    // ============================================================
    // PAYLOAD
    // ============================================================

    let payload = json!({
        "from": config.mail_from,
        "to": [to],
        "subject": subject,
        "text": body
    });

    // ============================================================
    // ENVIO PARA RESEND
    // ============================================================

    let response = client
        .post("https://api.resend.com/emails")
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    // ============================================================
    // RESPOSTA
    // ============================================================

    let status = response.status();
    let response_body = response.text().await?;

    if !status.is_success() {
        tracing::error!(
            "[resend] erro ao enviar e-mail: status={} resposta={}",
            status,
            response_body
        );

        anyhow::bail!(
            "Resend retornou HTTP {}: {}",
            status,
            response_body
        );
    }

    tracing::info!(
        "[resend] e-mail enviado com sucesso para={} resposta={}",
        to,
        response_body
    );

    Ok(())
}

/// Corpo do e-mail de verificação.
pub fn verification_email_body(
    frontend_url: &str,
    token: &str,
) -> String {
    format!(
        "Confirme seu e-mail acessando o link abaixo:\n\n\
         {frontend_url}/verificar-email?token={token}\n\n\
         Esse link expira em 24 horas."
    )
}

/// Corpo do e-mail de recuperação de senha.
pub fn password_reset_email_body(
    frontend_url: &str,
    token: &str,
) -> String {
    format!(
        "Recebemos um pedido para redefinir sua senha.\n\n\
         Acesse o link abaixo:\n\n\
         {frontend_url}/redefinir-senha?token={token}\n\n\
         Se você não pediu isso, ignore este e-mail.\n\
         O link expira em 1 hora."
    )
}