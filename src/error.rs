use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("session not found")]
    SessionNotFound,
    #[error("session is not open for booking")]
    SessionNotBookable,
    #[error("session is full")]
    SessionFull,
    #[error("no valid credit available")]
    NoValidCredit,
    #[error("already booked this session")]
    AlreadyBooked,
    #[error("booking not found")]
    BookingNotFound,
    #[error("booking cannot be cancelled")]
    NotCancellable,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("resource already exists")]
    Conflict,
    #[error("integration error: {0}")]
    Integration(&'static str),
    // Dữ liệu DB không hợp lệ (vd category lạ) — lỗi của mình, không phải user.
    #[error("corrupt data: {0}")]
    Corrupt(&'static str),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::SessionNotFound | AppError::BookingNotFound => {
                (StatusCode::NOT_FOUND, "not_found")
            }
            AppError::SessionNotBookable => (StatusCode::CONFLICT, "session_not_bookable"),
            AppError::SessionFull => (StatusCode::CONFLICT, "session_full"),
            AppError::NoValidCredit => (StatusCode::UNPROCESSABLE_ENTITY, "no_valid_credit"),
            AppError::AlreadyBooked => (StatusCode::CONFLICT, "already_booked"),
            AppError::NotCancellable => (StatusCode::CONFLICT, "not_cancellable"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::InvalidInput(_) => (StatusCode::BAD_REQUEST, "invalid_input"),
            AppError::Conflict => (StatusCode::CONFLICT, "conflict"),
            AppError::Integration(_) => (StatusCode::INTERNAL_SERVER_ERROR, "integration_error"),
            AppError::Corrupt(what) => {
                tracing::error!(field = what, "corrupt data in db");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
            AppError::Db(e) => {
                tracing::error!(error = ?e, "db error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        let body = Json(json!({
            "error": { "code": code, "message": self.to_string() }
        }));
        (status, body).into_response()
    }
}
