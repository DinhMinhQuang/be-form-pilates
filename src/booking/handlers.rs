// Handler CHỈ làm: lấy input từ request, gọi service, gói response. Không có logic nghiệp vụ.
// Đây là phần "C" của MVC nhưng giữ thật mỏng.
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;
use uuid::Uuid;

use super::service;
use crate::auth::AuthStudent;
use crate::domain::BookingChannel;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct BookResponse {
    pub booking_id: Uuid,
}

#[derive(Serialize)]
pub struct CancelResponse {
    pub refunded: bool,
}

// POST /sessions/:id/book
pub async fn book(
    State(st): State<AppState>,
    student: AuthStudent,
    Path(session_id): Path<Uuid>,
) -> Result<(StatusCode, Json<BookResponse>), AppError> {
    let booking_id = service::book_class(
        &st.pool,
        student.0,
        session_id,
        student.0,
        BookingChannel::Student,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(BookResponse { booking_id })))
}

// POST /bookings/:id/cancel
pub async fn cancel(
    State(st): State<AppState>,
    student: AuthStudent,
    Path(booking_id): Path<Uuid>,
) -> Result<Json<CancelResponse>, AppError> {
    let out = service::cancel_booking(&st.pool, booking_id, student.0, false).await?;
    Ok(Json(CancelResponse {
        refunded: out.refunded,
    }))
}
