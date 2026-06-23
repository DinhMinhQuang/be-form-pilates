use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router, async_trait,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    routing::post,
};
use chrono::{Duration, Utc};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{error::AppError, state::AppState};

const MAGIC_LINK_TTL_MINUTES: i64 = 20;
const SESSION_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Student,
    Trainer,
    Admin,
}

impl Role {
    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "student" => Ok(Self::Student),
            "trainer" => Ok(Self::Trainer),
            "admin" => Ok(Self::Admin),
            _ => Err(AppError::Corrupt("user.role")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: Role,
}

pub struct AuthStudent(pub Uuid);
pub struct AuthTrainer(pub Uuid);
pub struct AuthAdmin(pub Uuid);

pub(crate) fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub(crate) fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;

        let row: Option<(Uuid, String)> = sqlx::query_as(
            r#"SELECT u.id, u.role
               FROM auth_session s
               JOIN app_user u ON u.id = s.user_id
               WHERE s.token_hash = $1
                 AND s.expires_at > now()
                 AND s.revoked_at IS NULL
                 AND u.status = 'active'"#,
        )
        .bind(token_hash(token))
        .fetch_optional(&state.pool)
        .await?;

        let (id, role) = row.ok_or(AppError::Unauthorized)?;
        Ok(Self {
            id,
            role: Role::from_db(&role)?,
        })
    }
}

macro_rules! role_extractor {
    ($name:ident, $role:pat) => {
        #[async_trait]
        impl FromRequestParts<AppState> for $name {
            type Rejection = AppError;

            async fn from_request_parts(
                parts: &mut Parts,
                state: &AppState,
            ) -> Result<Self, Self::Rejection> {
                let user = AuthUser::from_request_parts(parts, state).await?;
                if !matches!(user.role, $role) {
                    return Err(AppError::Forbidden);
                }
                Ok(Self(user.id))
            }
        }
    };
}

role_extractor!(AuthStudent, Role::Student);
role_extractor!(AuthTrainer, Role::Trainer | Role::Admin);
role_extractor!(AuthAdmin, Role::Admin);

#[derive(Deserialize)]
struct MagicLinkRequest {
    email: Option<String>,
    phone: Option<String>,
}

#[derive(Serialize)]
struct MagicLinkResponse {
    accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    dev_token: Option<String>,
}

#[derive(Deserialize)]
struct ExchangeRequest {
    token: String,
}

#[derive(Deserialize)]
struct StaffLoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct SessionResponse {
    access_token: String,
    token_type: &'static str,
    expires_at: chrono::DateTime<Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/magic-links", post(request_magic_link))
        .route("/auth/magic/exchange", post(exchange_magic_link))
        .route("/auth/staff/login", post(staff_login))
}

async fn request_magic_link(
    State(state): State<AppState>,
    Json(input): Json<MagicLinkRequest>,
) -> Result<(StatusCode, Json<MagicLinkResponse>), AppError> {
    let email = input.email.map(|value| value.trim().to_lowercase());
    let phone = input.phone.map(|value| normalize_phone(&value));
    if email.is_none() && phone.is_none() {
        return Err(AppError::InvalidInput("email_or_phone_required"));
    }

    let student: Option<(Uuid, Option<String>)> = sqlx::query_as(
        r#"SELECT id, email FROM app_user
           WHERE role = 'student' AND status = 'active'
             AND (($1::text IS NOT NULL AND lower(email) = $1)
               OR ($2::text IS NOT NULL AND phone = $2))"#,
    )
    .bind(email)
    .bind(phone)
    .fetch_optional(&state.pool)
    .await?;

    let mut dev_token = None;
    if let Some((student_id, recipient)) = student {
        let mut tx = state.pool.begin().await?;
        let token = issue_magic_link(&mut tx, student_id).await?;
        if let Some(recipient) = recipient {
            let base_url = std::env::var("MAGIC_LINK_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3000/auth/magic".to_owned());
            sqlx::query("INSERT INTO email_outbox (recipient, template, payload) VALUES ($1, 'magic_link', $2)")
                .bind(recipient)
                .bind(serde_json::json!({"url": format!("{base_url}?token={token}")}))
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        tracing::info!(student_id = %student_id, "magic link issued; hand token to configured email provider");
        if std::env::var("EXPOSE_MAGIC_TOKEN").as_deref() == Ok("true") {
            dev_token = Some(token);
        }
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(MagicLinkResponse {
            accepted: true,
            dev_token,
        }),
    ))
}

async fn exchange_magic_link(
    State(state): State<AppState>,
    Json(input): Json<ExchangeRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    let mut tx = state.pool.begin().await?;
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT id, student_id FROM magic_link
           WHERE token_hash = $1 AND used_at IS NULL AND expires_at > now()
           FOR UPDATE"#,
    )
    .bind(token_hash(&input.token))
    .fetch_optional(&mut *tx)
    .await?;
    let (link_id, student_id) = row.ok_or(AppError::Unauthorized)?;
    sqlx::query("UPDATE magic_link SET used_at = now() WHERE id = $1")
        .bind(link_id)
        .execute(&mut *tx)
        .await?;
    let response = create_session(&mut tx, student_id).await?;
    tx.commit().await?;
    Ok(Json(response))
}

async fn staff_login(
    State(state): State<AppState>,
    Json(input): Json<StaffLoginRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        r#"SELECT u.id, c.password_hash
           FROM app_user u JOIN staff_credential c ON c.user_id = u.id
           WHERE lower(u.email) = lower($1) AND u.role IN ('trainer', 'admin') AND u.status = 'active'"#,
    )
    .bind(input.email.trim())
    .fetch_optional(&state.pool)
    .await?;
    let (user_id, hash) = row.ok_or(AppError::Unauthorized)?;
    let parsed = PasswordHash::new(&hash).map_err(|_| AppError::Corrupt("password_hash"))?;
    Argon2::default()
        .verify_password(input.password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized)?;

    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE staff_credential SET last_login_at = now() WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let response = create_session(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(Json(response))
}

async fn create_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<SessionResponse, AppError> {
    let token = random_token();
    let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query("INSERT INTO auth_session (user_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(token_hash(&token))
        .bind(expires_at)
        .execute(&mut **tx)
        .await?;
    Ok(SessionResponse {
        access_token: token,
        token_type: "Bearer",
        expires_at,
    })
}

pub fn normalize_phone(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

pub(crate) async fn issue_magic_link(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    student_id: Uuid,
) -> Result<String, AppError> {
    let token = random_token();
    sqlx::query("INSERT INTO magic_link (student_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(student_id)
        .bind(token_hash(&token))
        .bind(Utc::now() + Duration::minutes(MAGIC_LINK_TTL_MINUTES))
        .execute(&mut **tx)
        .await?;
    Ok(token)
}
