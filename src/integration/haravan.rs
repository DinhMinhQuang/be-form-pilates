use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::{auth, error::AppError, state::AppState};

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
struct HaravanOrder {
    id: Value,
    #[serde(default)]
    financial_status: String,
    paid_at: Option<DateTime<Utc>>,
    processed_at: Option<DateTime<Utc>>,
    email: Option<String>,
    phone: Option<String>,
    customer: Option<HaravanCustomer>,
    #[serde(default)]
    line_items: Vec<HaravanLineItem>,
}

#[derive(Deserialize)]
struct HaravanCustomer {
    id: Option<Value>,
    email: Option<String>,
    phone: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Deserialize)]
struct HaravanLineItem {
    variant_id: Option<Value>,
    #[serde(default = "one")]
    quantity: i32,
}

fn one() -> i32 {
    1
}

fn value_id(value: &Value) -> Result<String, AppError> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(AppError::InvalidInput("invalid_haravan_id")),
    }
}

fn verify_signature(body: &[u8], signature: &str, secret: &str) -> bool {
    let Ok(signature) = STANDARD.decode(signature) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&signature).is_ok()
}

pub async fn order_paid(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    let secret = std::env::var("HARAVAN_WEBHOOK_SECRET")
        .map_err(|_| AppError::Integration("HARAVAN_WEBHOOK_SECRET_missing"))?;
    let signature = headers
        .get("x-haravan-hmacsha256")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    if !verify_signature(&body, signature, &secret) {
        return Err(AppError::Unauthorized);
    }

    let payload: Value =
        serde_json::from_slice(&body).map_err(|_| AppError::InvalidInput("invalid_json"))?;
    let order: HaravanOrder = serde_json::from_value(payload.clone())
        .map_err(|_| AppError::InvalidInput("invalid_haravan_order"))?;
    let external_order_id = value_id(&order.id)?;
    let event_key = format!("order_paid:{external_order_id}");

    let mut tx = state.pool.begin().await?;
    let event: Option<(Uuid,)> = sqlx::query_as(
        r#"INSERT INTO integration_event
             (provider, external_event_id, event_type, payload, signature_valid, status, attempts)
           VALUES ('haravan', $1, 'orders/paid', $2, true, 'processing', 1)
           ON CONFLICT (provider, external_event_id) DO NOTHING RETURNING id"#,
    )
    .bind(&event_key)
    .bind(&payload)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((integration_event_id,)) = event else {
        return Ok(StatusCode::OK);
    };
    if order.financial_status != "paid" {
        return Err(AppError::InvalidInput("order_not_paid"));
    }

    let customer = order.customer.as_ref();
    let email = customer
        .and_then(|value| value.email.clone())
        .or(order.email)
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty());
    let phone = customer
        .and_then(|value| value.phone.clone())
        .or(order.phone)
        .map(|value| auth::normalize_phone(&value))
        .filter(|value| !value.is_empty());
    if email.is_none() && phone.is_none() {
        return Err(AppError::InvalidInput("customer_identity_required"));
    }
    let haravan_customer_id = customer
        .and_then(|value| value.id.as_ref())
        .map(value_id)
        .transpose()?;
    let full_name = customer
        .map(|value| {
            format!(
                "{} {}",
                value.first_name.as_deref().unwrap_or(""),
                value.last_name.as_deref().unwrap_or("")
            )
            .trim()
            .to_owned()
        })
        .unwrap_or_default();

    let existing: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT u.id FROM app_user u LEFT JOIN student_profile sp ON sp.user_id = u.id
           WHERE u.role = 'student' AND (
             ($1::text IS NOT NULL AND sp.haravan_customer_id = $1) OR
             ($2::text IS NOT NULL AND lower(u.email) = $2) OR
             ($3::text IS NOT NULL AND u.phone = $3))
           LIMIT 1 FOR UPDATE OF u"#,
    )
    .bind(&haravan_customer_id)
    .bind(&email)
    .bind(&phone)
    .fetch_optional(&mut *tx)
    .await?;
    let student_id = if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE app_user SET email = COALESCE(email, $2), phone = COALESCE(phone, $3), full_name = CASE WHEN full_name = '' THEN $4 ELSE full_name END, updated_at = now() WHERE id = $1",
        ).bind(id).bind(&email).bind(&phone).bind(&full_name).execute(&mut *tx).await?;
        id
    } else {
        let (id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO app_user (role, email, phone, full_name) VALUES ('student', $1, $2, $3) RETURNING id",
        ).bind(&email).bind(&phone).bind(&full_name).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO student_profile (user_id, haravan_customer_id) VALUES ($1, $2)")
            .bind(id)
            .bind(&haravan_customer_id)
            .execute(&mut *tx)
            .await?;
        id
    };

    let activated_at = order
        .paid_at
        .or(order.processed_at)
        .unwrap_or_else(Utc::now);
    let (order_id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO customer_order
             (provider, external_order_id, student_id, financial_status, paid_at, raw_payload)
           VALUES ('haravan', $1, $2, 'paid', $3, $4) RETURNING id"#,
    )
    .bind(&external_order_id)
    .bind(student_id)
    .bind(activated_at)
    .bind(&payload)
    .fetch_one(&mut *tx)
    .await?;

    let mut credited_items = 0;
    for item in order.line_items {
        let Some(variant) = item.variant_id.as_ref() else {
            continue;
        };
        let variant_id = value_id(variant)?;
        let mapping: Option<(Uuid, i32, i32, Option<Uuid>)> = sqlx::query_as(
            r#"SELECT cp.id, cp.sessions, cp.validity_days, m.branch_id
               FROM haravan_product_mapping m JOIN course_package cp ON cp.id = m.package_id
               WHERE m.haravan_variant_id = $1 AND m.active AND cp.status = 'active'"#,
        )
        .bind(&variant_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((package_id, sessions, validity_days, branch_id)) = mapping else {
            tracing::warn!(variant_id, "Haravan variant has no active package mapping");
            continue;
        };
        if item.quantity <= 0 {
            return Err(AppError::InvalidInput("invalid_quantity"));
        }
        let (order_item_id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO order_item (order_id, external_variant_id, package_id, quantity) VALUES ($1, $2, $3, $4) RETURNING id",
        ).bind(order_id).bind(&variant_id).bind(package_id).bind(item.quantity)
            .fetch_one(&mut *tx).await?;
        let total_sessions = sessions
            .checked_mul(item.quantity)
            .ok_or(AppError::InvalidInput("quantity_too_large"))?;
        let expires_at = if validity_days > 0 {
            activated_at + Duration::days(validity_days as i64 * item.quantity as i64)
        } else {
            activated_at + Duration::days(36500) // 100 năm = không hết hạn
        };
        let (lot_id,): (Uuid,) = sqlx::query_as(
            r#"INSERT INTO credit_lot
                 (student_id, package_id, order_item_id, sessions_total, sessions_remaining, activated_at, expires_at, branch_id)
               VALUES ($1, $2, $3, $4, $4, $5, $6, $7) RETURNING id"#,
        ).bind(student_id).bind(package_id).bind(order_item_id).bind(total_sessions)
            .bind(activated_at).bind(expires_at).bind(branch_id).fetch_one(&mut *tx).await?;
        sqlx::query(
            r#"INSERT INTO credit_ledger
                 (student_id, lot_id, integration_event_id, delta, balance_after, reason, metadata)
               VALUES ($1, $2, $3, $4, $4, 'haravan_purchase', $5)"#,
        )
        .bind(student_id)
        .bind(lot_id)
        .bind(integration_event_id)
        .bind(total_sessions)
        .bind(json!({"external_order_id": external_order_id, "variant_id": variant_id}))
        .execute(&mut *tx)
        .await?;
        credited_items += 1;
    }
    if credited_items == 0 {
        return Err(AppError::Integration("no_mapped_course_package"));
    }

    if let Some(recipient) = email {
        let magic_token = auth::issue_magic_link(&mut tx, student_id).await?;
        let base_url = std::env::var("MAGIC_LINK_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000/auth/magic".to_owned());
        sqlx::query("INSERT INTO email_outbox (recipient, template, payload) VALUES ($1, 'order_confirmed_magic_link', $2)")
            .bind(recipient).bind(json!({"url": format!("{base_url}?token={magic_token}"), "external_order_id": external_order_id}))
            .execute(&mut *tx).await?;
    }
    sqlx::query(
        "UPDATE integration_event SET status = 'processed', processed_at = now() WHERE id = $1",
    )
    .bind(integration_event_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct HaravanVerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: String,
    #[serde(rename = "hub.verify_token")]
    verify_token: String,
    #[serde(rename = "hub.challenge")]
    challenge: String,
}

pub async fn verify_webhook(
    State(_state): State<AppState>,
    Query(query): Query<HaravanVerifyQuery>,
) -> Result<String, AppError> {
    let secret = std::env::var("HARAVAN_WEBHOOK_SECRET")
        .map_err(|_| AppError::Integration("HARAVAN_WEBHOOK_SECRET_missing"))?;
    if query.mode != "subscribe" || query.verify_token != secret {
        return Err(AppError::Unauthorized);
    }
    Ok(query.challenge)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_haravan_hmac() {
        let body = br#"{"id":1}"#;
        let mut mac = HmacSha256::new_from_slice(b"secret").unwrap();
        mac.update(body);
        let signature = STANDARD.encode(mac.finalize().into_bytes());
        assert!(verify_signature(body, &signature, "secret"));
        assert!(!verify_signature(body, &signature, "wrong"));
    }
}
