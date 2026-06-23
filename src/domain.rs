// Type cho cả app dùng chung. Newtype/enum thay vì String trôi nổi —
// rẻ, type-safe, đúng điểm mạnh Rust. Map sang text ngay tại biên DB (queries.rs).
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookingStatus {
    Booked,
    CancelledRefunded,
    Attended,
    NoShow,
}

impl BookingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BookingStatus::Booked => "booked",
            BookingStatus::CancelledRefunded => "cancelled_refunded",
            BookingStatus::Attended => "attended",
            BookingStatus::NoShow => "no_show",
        }
    }
}

// Ai bấm nút: student tự đặt, hay admin đặt dùm. Cùng một service.
// Admin chưa dùng trong slice này (sẽ dùng ở feature admin) nên allow dead_code.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum BookingChannel {
    Student,
    Admin,
}

impl BookingChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            BookingChannel::Student => "student",
            BookingChannel::Admin => "admin",
        }
    }
}
