use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// Enqueue both channels in the caller's transaction, sharing one single-use link.
pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    email: Option<&str>,
    phone: Option<&str>,
    template: &str,
    payload: Value,
) -> Result<(), sqlx::Error> {
    if let Some(email) = email.filter(|value| !value.trim().is_empty()) {
        sqlx::query("INSERT INTO email_outbox (recipient, template, payload) VALUES ($1, $2, $3)")
            .bind(email)
            .bind(template)
            .bind(&payload)
            .execute(&mut **tx)
            .await?;
    }
    if let Some(phone) = phone.filter(|value| !value.trim().is_empty()) {
        let payload = crate::zns::template_payload(template, &payload);
        sqlx::query("INSERT INTO sms_outbox (recipient, template, payload) VALUES ($1, $2, $3)")
            .bind(phone)
            .bind(template)
            .bind(&payload)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
