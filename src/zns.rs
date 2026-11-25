use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const ENDPOINT: &str = "https://business.openapi.zalo.me/message/template";
pub(crate) fn template_enabled(name: &str) -> bool {
    template_id(name).is_ok() && crate::zns_token::Credentials::from_env().is_ok()
}

fn template_id(name: &str) -> anyhow::Result<String> {
    let variable = match name {
        "magic_link" => "ZNS_MAGIC_LINK_TEMPLATE_ID",
        "order_confirmed_magic_link" => "ZNS_ORDER_CONFIRMED_TEMPLATE_ID",
        _ => anyhow::bail!("unsupported ZNS template"),
    };
    let value = std::env::var(variable)?;
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "missing {variable}");
    Ok(value.to_owned())
}

pub async fn start_worker(pool: PgPool) {
    let credentials = match crate::zns_token::Credentials::from_env() {
        Ok(credentials) => credentials,
        Err(_) => {
            tracing::warn!("ZNS_APP_ID or ZNS_APP_SECRET missing; ZNS worker disabled");
            return;
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::error!(%error, "ZNS client initialization failed");
            return;
        }
    };
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    loop {
        interval.tick().await;
        let token = match crate::zns_token::access_token(&pool, &client, &credentials).await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, "ZNS access token unavailable; queued messages retained");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        if let Err(error) = flush(&pool, &client, &token).await {
            tracing::error!(%error, "ZNS outbox flush failed");
        }
    }
}

async fn flush(pool: &PgPool, client: &reqwest::Client, token: &str) -> anyhow::Result<()> {
    let enabled_templates: Vec<&str> = ["magic_link", "order_confirmed_magic_link"]
        .into_iter()
        .filter(|name| template_id(name).is_ok())
        .collect();
    // Lock through delivery and status update to prevent concurrent workers sending
    // the same row. tracking_id is for reconciliation, not guaranteed deduplication.
    for _ in 0..20 {
        let mut tx = pool.begin().await?;
        let row: Option<(Uuid, String, String, Value)> = sqlx::query_as(
            "SELECT id, recipient, template, payload FROM sms_outbox \
             WHERE status = 'pending' AND attempts < 3 AND template = ANY($1) \
             ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED",
        )
        .bind(&enabled_templates)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((id, recipient, template, payload)) = row else {
            break;
        };
        let result = async {
            let body = build_request(id, &recipient, &template, &payload)?;
            send(client, ENDPOINT, token, &body).await
        }
        .await;
        sqlx::query(
            "UPDATE sms_outbox SET attempts = attempts + 1, \
             status = CASE WHEN $2 THEN 'sent' WHEN attempts + 1 >= 3 THEN 'failed' ELSE 'pending' END, \
             sent_at = CASE WHEN $2 THEN now() ELSE NULL END WHERE id = $1",
        ).bind(id).bind(result.is_ok()).execute(&mut *tx).await?;
        tx.commit().await?;
        match result {
            Ok(message_id) => tracing::info!(%id, %message_id, "ZNS accepted"),
            Err(error) => {
                tracing::warn!(%id, %error, "ZNS delivery failed");
                break;
            }
        }
    }
    Ok(())
}

pub(crate) fn normalize_phone(raw: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        raw.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '-' | '(' | ')')),
        "invalid ZNS phone characters"
    );
    let digits = crate::auth::normalize_phone(raw);
    let phone = if let Some(local) = digits.strip_prefix('0') {
        format!("84{local}")
    } else {
        digits
    };
    anyhow::ensure!(
        phone.starts_with("84") && phone.len() == 11 && phone.as_bytes()[2] != b'0',
        "invalid Vietnamese ZNS phone"
    );
    Ok(phone)
}

fn template_fields(template: &str) -> anyhow::Result<&'static [&'static str]> {
    let fields: &[&str] = match template {
        "magic_link" => &["name", "student_code", "login_token"],
        "order_confirmed_magic_link" => &[
            "name",
            "order_code",
            "phone_number",
            "price",
            "status",
            "date",
            "login_token",
        ],
        _ => anyhow::bail!("unsupported ZNS template"),
    };
    Ok(fields)
}

pub(crate) fn template_payload(template: &str, payload: &Value) -> Value {
    let mut data = serde_json::Map::new();
    if let Ok(fields) = template_fields(template) {
        for &field in fields {
            if let Some(value) = payload.get(field) {
                data.insert(field.to_owned(), value.clone());
            }
        }
    }
    Value::Object(data)
}

fn build_request(
    id: Uuid,
    recipient: &str,
    template: &str,
    payload: &Value,
) -> anyhow::Result<Value> {
    let template_id = template_id(template)?;
    let fields = template_fields(template)?;
    let mut data = serde_json::Map::new();
    for &field in fields {
        let value = payload[field]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing ZNS payload field: {field}"))?;
        data.insert(field.to_owned(), json!(value));
    }
    Ok(json!({
        "phone": normalize_phone(recipient)?,
        "template_id": template_id,
        "template_data": data,
        "tracking_id": id.simple().to_string(),
    }))
}

#[derive(Deserialize)]
struct SendResponse {
    error: i64,
    #[serde(default)]
    message: String,
    data: Option<Value>,
}

#[derive(Deserialize)]
struct SendData {
    msg_id: String,
}

async fn send(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    body: &Value,
) -> anyhow::Result<String> {
    let started = std::time::Instant::now();
    let tracking_id = body["tracking_id"].as_str().unwrap_or_default();
    let template_id = body["template_id"].as_str().unwrap_or_default();
    tracing::info!(tracking_id, template_id, "ZNS request started");
    let response = client
        .post(endpoint)
        .header("access_token", token)
        .json(body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("ZNS request transport failed"))?;
    tracing::info!(
        tracking_id,
        template_id,
        status = response.status().as_u16(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "ZNS response received"
    );
    let status = response.status();
    let response_json: Value = response.json().await.map_err(|_| {
        anyhow::anyhow!("ZNS HTTP {status}: response is not a valid ZNS JSON response")
    })?;
    tracing::info!(
        tracking_id,
        template_id,
        http_status = status.as_u16(),
        response = %response_json,
        "ZNS response body"
    );
    let response: SendResponse = serde_json::from_value(response_json)
        .map_err(|_| anyhow::anyhow!("ZNS HTTP {status}: invalid ZNS response fields"))?;
    if !status.is_success() || response.error != 0 {
        tracing::warn!(
            tracking_id,
            template_id,
            http_status = status.as_u16(),
            error_code = response.error,
            provider_message = %response.message,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "ZNS request rejected"
        );
        anyhow::bail!(
            "ZNS HTTP {status}, API error {}: {}",
            response.error,
            response.message
        );
    }
    let data: SendData = serde_json::from_value(
        response
            .data
            .ok_or_else(|| anyhow::anyhow!("ZNS response missing data"))?,
    )
    .map_err(|_| anyhow::anyhow!("ZNS response missing message ID"))?;
    anyhow::ensure!(!data.msg_id.is_empty(), "ZNS response missing message ID");
    Ok(data.msg_id)
}
