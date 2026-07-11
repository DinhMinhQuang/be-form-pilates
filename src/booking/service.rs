// Đây là "chỗ thứ ba" mà MVC thuần thiếu: không phải transport, không phải data struct.
// Mọi invariant của booking enforce bằng transaction + lock + constraint DB, KHÔNG dựng
// aggregate trong RAM. Handler student và admin-book-dùm đều gọi chung hai hàm dưới đây.
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::queries;
use crate::domain::{BookingChannel, BookingStatus};
use crate::error::AppError;

const CANCEL_WINDOW: Duration = Duration::hours(6);

pub struct CancelOutcome {
    pub refunded: bool,
}

// BEGIN → lock session → 3 guard → chọn lot → debit → commit.
pub async fn book_class(
    pool: &PgPool,
    student_id: Uuid,
    session_id: Uuid,
    booked_by: Uuid,
    channel: BookingChannel,
) -> Result<Uuid, AppError> {
    let mut tx = pool.begin().await?;

    // 1. Khóa dòng session để serialize việc đếm slot.
    let session = queries::lock_session(&mut tx, session_id)
        .await?
        .ok_or(AppError::SessionNotFound)?;
    if session.status != "scheduled" {
        return Err(AppError::SessionNotBookable);
    }
    if session.start_at <= Utc::now() {
        return Err(AppError::SessionNotBookable);
    }
    if session.booked_count >= session.capacity {
        return Err(AppError::SessionFull);
    }

    // 2. Khóa lot sẽ trừ. Không có lot hợp lệ = hết buổi, hết hạn, hoặc sai chi nhánh.
    let lot_id = queries::pick_credit_lot(
        &mut tx,
        student_id,
        session.class_type_id,
        session.branch_id,
        session.start_at,
    )
    .await?
    .ok_or(AppError::NoValidCredit)?;

    // 3. Insert booking. Unique(session_id, student_id) là lớp chặn cuối cho double-book.
    let booking_id =
        match queries::insert_booking(&mut tx, session_id, student_id, lot_id, booked_by, channel)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                if e.as_database_error().map(|d| d.is_unique_violation()) == Some(true) {
                    return Err(AppError::AlreadyBooked);
                }
                return Err(e.into());
            }
        };

    // 4. Debit + đếm slot + ghi sổ. Tất cả trong cùng transaction.
    let (_, balance) = queries::adjust_lot(&mut tx, lot_id, -1).await?;
    queries::adjust_session_count(&mut tx, session_id, 1).await?;
    queries::write_ledger(
        &mut tx, lot_id, booking_id, -1, "book", booked_by, student_id, balance,
    )
    .await?;

    tx.commit().await?;
    Ok(booking_id)
}

// Hủy: >= 6h trước giờ học thì hoàn +1 VỀ ĐÚNG lot cũ; trong 6h thì khóa, không hoàn.
pub async fn cancel_booking(
    pool: &PgPool,
    booking_id: Uuid,
    actor_id: Uuid,
    allow_any_student: bool,
) -> Result<CancelOutcome, AppError> {
    let mut tx = pool.begin().await?;

    let b = queries::lock_booking(&mut tx, booking_id)
        .await?
        .ok_or(AppError::BookingNotFound)?;
    if b.status != "booked" {
        return Err(AppError::NotCancellable);
    }
    if !allow_any_student && b.student_id != actor_id {
        return Err(AppError::BookingNotFound);
    }

    let refundable = b.start_at - Utc::now() >= CANCEL_WINDOW;
    if !refundable {
        return Err(AppError::NotCancellable);
    }
    // Hoàn về lot ban đầu (b.credit_lot_id), KHÔNG phải lot nearest-expiry.
    let (_, balance) = queries::adjust_lot(&mut tx, b.credit_lot_id, 1).await?;
    queries::write_ledger(
        &mut tx,
        b.credit_lot_id,
        booking_id,
        1,
        "cancel_refund",
        actor_id,
        b.student_id,
        balance,
    )
    .await?;
    let new_status = BookingStatus::CancelledRefunded;

    queries::set_booking_status(&mut tx, booking_id, new_status).await?;
    queries::adjust_session_count(&mut tx, b.session_id, -1).await?;

    tx.commit().await?;
    Ok(CancelOutcome { refunded: true })
}
