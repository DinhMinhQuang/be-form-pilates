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
    #[error("student already has a booking in this time range")]
    ScheduleConflict,
    #[error("student {0} already has a booking in this time range")]
    ScheduleConflictNamed(String),
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
    #[error("corrupt data: {0}")]
    Corrupt(&'static str),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

fn vi_message(code: &str) -> &'static str {
    match code {
        // Session
        "not_found" => "Không tìm thấy.",
        "session_not_bookable" => "Buổi học không thể đặt lịch.",
        "session_full" => "Buổi học đã đầy chỗ.",
        "already_booked" => "Bạn đã đặt lịch buổi học này rồi.",
        "schedule_conflict" => "Bạn đã có lịch học trong khoảng thời gian này.",
        "not_cancellable" => "Không thể hủy — đã qua thời hạn hủy 6 tiếng trước buổi học.",
        // Booking / credit
        "no_valid_credit" => {
            "Không tìm thấy buổi tập hợp lệ. Vui lòng kiểm tra gói tập hoặc chi nhánh."
        }
        "conflict" => "Xung đột dữ liệu, vui lòng thử lại.",
        // Auth
        "unauthorized" => "Chưa đăng nhập hoặc phiên đã hết hạn.",
        "forbidden" => "Bạn không có quyền thực hiện thao tác này.",
        // Validation
        "invalid_session_time" => "Thời gian buổi học không hợp lệ.",
        "invalid_session_update" => "Không thể cập nhật buổi học với thông tin này.",
        "invalid_session_status" => "Trạng thái buổi học không hợp lệ.",
        "invalid_capacity" => "Sức chứa không hợp lệ (1–6).",
        "invalid_trainer" => "Giáo viên không tồn tại hoặc đã bị vô hiệu.",
        "invalid_status" => "Trạng thái không hợp lệ.",
        "invalid_booking_range" => "Khoảng thời gian tìm kiếm không hợp lệ.",
        "invalid_schedule_range" => "Khoảng thời gian lịch học không hợp lệ.",
        "invalid_credit_delta" => "Số buổi điều chỉnh không hợp lệ.",
        "invalid_attendance_status" => "Trạng thái điểm danh phải là 'attended' hoặc 'no_show'.",
        "invalid_quantity" => "Số lượng không hợp lệ.",
        "invalid_json" => "Dữ liệu gửi lên không đúng định dạng.",
        "invalid_haravan_order" => "Dữ liệu đơn hàng Haravan không hợp lệ.",
        "class_not_available_at_branch" => "Loại lớp học này không có tại chi nhánh.",
        "email_or_phone_required" => "Vui lòng cung cấp email hoặc số điện thoại.",
        "adjustment_and_reason_required" => "Cần có lý do và ít nhất một thay đổi (buổi hoặc hạn).",
        "insufficient_credit" => "Số buổi còn lại không đủ để điều chỉnh.",
        "quantity_too_large" => "Số lượng vượt quá giới hạn cho phép.",
        "query_too_short" => "Từ khóa tìm kiếm cần ít nhất 2 ký tự.",
        "order_not_paid" => "Đơn hàng chưa được thanh toán.",
        "trainer_schedule_conflict" => "Huấn luyện viên đã có lịch dạy trong khoảng thời gian này.",
        // Integration / internal
        "integration_error" => "Lỗi tích hợp, vui lòng thử lại sau.",
        "internal" => "Lỗi hệ thống, vui lòng thử lại sau.",
        _ => "Yêu cầu không hợp lệ.",
    }
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
            AppError::ScheduleConflict => (StatusCode::CONFLICT, "schedule_conflict"),
            AppError::ScheduleConflictNamed(name) => {
                let body = Json(json!({
                    "error": { "code": "schedule_conflict", "message": format!("Học viên {} đã có lịch học trong khung giờ này.", name) }
                }));
                return (StatusCode::CONFLICT, body).into_response();
            }
            AppError::NotCancellable => (StatusCode::CONFLICT, "not_cancellable"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::InvalidInput(key) => {
                let body = Json(json!({
                    "error": { "code": key, "message": vi_message(key) }
                }));
                return (StatusCode::BAD_REQUEST, body).into_response();
            }
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
            "error": { "code": code, "message": vi_message(code) }
        }));
        (status, body).into_response()
    }
}
