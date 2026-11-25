use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgPool;

const ENDPOINT: &str = "https://oauth.zaloapp.com/v4/oa/access_token";

pub(crate) struct Credentials {
    pub app_id: String,
    pub secret: String,
}

impl Credentials {
    pub fn from_env() -> anyhow::Result<Self> {
        fn required(name: &str) -> anyhow::Result<String> {
            let value = std::env::var(name)?;
            anyhow::ensure!(!value.trim().is_empty(), "missing {name}");
            Ok(value)
        }
        Ok(Self {
            app_id: required("ZNS_APP_ID")?,
            secret: required("ZNS_APP_SECRET")?,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(alias = "expire_in")]
    expires_in: serde_json::Value,
}

async fn refresh(
    client: &reqwest::Client,
    endpoint: &str,
    credentials: &Credentials,
    refresh_token: &str,
) -> anyhow::Result<(String, String, DateTime<Utc>)> {
    // Never propagate provider bodies or transport errors containing credentials.
    let started = Utc::now();
    let response = client
        .post(endpoint)
        .header("secret_key", &credentials.secret)
        .form(&[
            ("app_id", credentials.app_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Zalo OAuth request failed"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "Zalo OAuth HTTP {}",
        response.status()
    );
    let token: TokenResponse = response.json().await.map_err(|_| {
        anyhow::anyhow!(
            "Zalo OAuth rejected refresh or returned invalid tokens; check OA authorization"
        )
    })?;
    let seconds = token
        .expires_in
        .as_i64()
        .or_else(|| token.expires_in.as_str().and_then(|s| s.parse().ok()))
        .filter(|seconds| *seconds > 300)
        .ok_or_else(|| anyhow::anyhow!("invalid Zalo token lifetime"))?;
    anyhow::ensure!(
        !token.access_token.trim().is_empty() && !token.refresh_token.trim().is_empty(),
        "empty Zalo OAuth tokens"
    );
    let expires_at = started
        .checked_add_signed(
            chrono::Duration::try_seconds(seconds)
                .ok_or_else(|| anyhow::anyhow!("invalid Zalo token lifetime"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("invalid Zalo token expiry"))?;
    Ok((token.access_token, token.refresh_token, expires_at))
}

pub(crate) async fn access_token(
    pool: &PgPool,
    client: &reqwest::Client,
    credentials: &Credentials,
) -> anyhow::Result<String> {
    stored_token(pool, client, credentials, ENDPOINT).await
}

async fn stored_token(
    pool: &PgPool,
    client: &reqwest::Client,
    credentials: &Credentials,
    endpoint: &str,
) -> anyhow::Result<String> {
    let mut tx = pool.begin().await?;
    // Serialize initialization as well as refresh across backend replicas.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("zalo-oauth:{}", credentials.app_id))
        .execute(&mut *tx)
        .await?;
    let row: Option<(String, String, DateTime<Utc>, Option<DateTime<Utc>>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT access_token, refresh_token, expires_at, refresh_expires_at, updated_at FROM zalo_oauth_tokens WHERE app_id = $1 FOR UPDATE"
    ).bind(&credentials.app_id).fetch_optional(&mut *tx).await?;
    let now = Utc::now();
    let vietnam = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let (access, refresh_token, expiry, refresh_expiry, updated_at) = row.ok_or_else(|| {
        anyhow::anyhow!(
            "Zalo OA authorization missing in database; authorize OA and save token pair"
        )
    })?;
    // A date change triggers one refresh after midnight Vietnam time, even without messages.
    // The DB lock and persisted updated_at deduplicate daily refresh across replicas/restarts.
    let daily_due =
        updated_at.with_timezone(&vietnam).date_naive() < now.with_timezone(&vietnam).date_naive();
    if expiry > now + chrono::Duration::minutes(5) && !daily_due {
        tx.commit().await?;
        return Ok(access);
    }
    anyhow::ensure!(
        refresh_expiry.is_none_or(|expiry| expiry > now),
        "Zalo OA refresh token expired; reauthorize OA and update token pair"
    );
    anyhow::ensure!(
        !refresh_token.trim().is_empty(),
        "Zalo OA refresh token missing in database; reauthorize OA and update token pair"
    );
    let (access, refresh, expiry) = refresh(client, endpoint, credentials, &refresh_token).await?;
    let refresh_expiry = Utc::now()
        .checked_add_months(chrono::Months::new(3))
        .ok_or_else(|| anyhow::anyhow!("invalid Zalo refresh token expiry"))?;
    sqlx::query("INSERT INTO zalo_oauth_tokens (app_id, access_token, refresh_token, expires_at, refresh_expires_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (app_id) DO UPDATE SET access_token = EXCLUDED.access_token, refresh_token = EXCLUDED.refresh_token, expires_at = EXCLUDED.expires_at, refresh_expires_at = EXCLUDED.refresh_expires_at, updated_at = now()")
        .bind(&credentials.app_id).bind(&access).bind(refresh).bind(expiry).bind(refresh_expiry).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(access)
}
