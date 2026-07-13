use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

fn build_mailer() -> Option<(AsyncSmtpTransport<Tokio1Executor>, String)> {
    let host = std::env::var("SMTP_HOST").ok()?;
    let username = std::env::var("SMTP_USERNAME").ok()?;
    let password = std::env::var("SMTP_PASSWORD").ok()?;
    let port: u16 = std::env::var("SMTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(465);
    let from = std::env::var("SMTP_FROM").unwrap_or_else(|_| username.clone());
    let creds = Credentials::new(username, password);
    let mailer = if port == 587 {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
    }
    .expect("failed to build SMTP transport")
    .port(port)
    .credentials(creds)
    .build();
    Some((mailer, from))
}

pub async fn start_worker(pool: PgPool) {
    let Some((mailer, from)) = build_mailer() else {
        tracing::warn!("GMAIL_USERNAME/GMAIL_APP_PASSWORD not set, email worker disabled");
        return;
    };
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        flush(&pool, &mailer, &from).await;
    }
}

async fn flush(pool: &PgPool, mailer: &AsyncSmtpTransport<Tokio1Executor>, from: &str) {
    let rows: Vec<(Uuid, String, String, Value)> = match sqlx::query_as(
        r#"SELECT id, recipient, template, payload
           FROM email_outbox
           WHERE status = 'pending' AND attempts < 3
           ORDER BY created_at
           LIMIT 20
           FOR UPDATE SKIP LOCKED"#,
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "email_outbox fetch failed");
            return;
        }
    };

    for (id, recipient, template, payload) in rows {
        let result = send_one(mailer, from, &recipient, &template, &payload).await;
        match result {
            Ok(_) => {
                let _ = sqlx::query(
                    "UPDATE email_outbox SET status = 'sent', sent_at = now(), attempts = attempts + 1 WHERE id = $1",
                )
                .bind(id)
                .execute(pool)
                .await;
                tracing::info!(template, recipient, "email sent");
            }
            Err(e) => {
                let _ = sqlx::query(
                    "UPDATE email_outbox SET status = CASE WHEN attempts + 1 >= 3 THEN 'failed' ELSE 'pending' END, attempts = attempts + 1 WHERE id = $1",
                )
                .bind(id)
                .execute(pool)
                .await;
                tracing::warn!(template, recipient, error = %e, "email send failed");
            }
        }
    }
}

async fn send_one(
    mailer: &AsyncSmtpTransport<Tokio1Executor>,
    from: &str,
    to: &str,
    template: &str,
    payload: &Value,
) -> anyhow::Result<()> {
    let (subject, body) = render(template, payload)?;
    let email = Message::builder()
        .from(from.parse()?)
        .to(to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body)?;
    mailer.send(email).await?;
    Ok(())
}

fn render(template: &str, payload: &Value) -> anyhow::Result<(String, String)> {
    match template {
        "magic_link" => {
            let url = payload["url"].as_str().unwrap_or("#");
            Ok((
                "Đăng nhập vào FORM Pilates".into(),
                format!(
                    r#"<p>Xin chào,</p>
<p>Nhấn vào liên kết bên dưới để đăng nhập vào tài khoản FORM Pilates của bạn:</p>
<p><a href="{url}">{url}</a></p>
<p>Liên kết có hiệu lực trong 15 phút và chỉ sử dụng được một lần.</p>
<p>Nếu bạn không yêu cầu liên kết này, vui lòng bỏ qua email này.</p>"#
                ),
            ))
        }
        "order_confirmed_magic_link" => {
            let url = payload["url"].as_str().unwrap_or("#");
            let order_id = payload["external_order_id"].as_str().unwrap_or("");
            Ok((
                "Xác nhận đơn hàng – FORM Pilates".into(),
                format!(
                    r#"<p>Cảm ơn bạn đã mua gói tập tại FORM Pilates!</p>
<p>Mã đơn hàng: <strong>{order_id}</strong></p>
<p>Buổi tập đã được cộng vào tài khoản của bạn. Nhấn vào liên kết bên dưới để đăng nhập và đặt lịch:</p>
<p><a href="{url}">{url}</a></p>
<p>Liên kết có hiệu lực trong 15 phút.</p>"#
                ),
            ))
        }
        _ => anyhow::bail!("unknown email template: {template}"),
    }
}
