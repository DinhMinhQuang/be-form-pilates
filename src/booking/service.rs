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

pub async fn sweep_completed_sessions(pool: &PgPool) -> Result<u64, AppError> {
    queries::sweep_completed_sessions(pool).await
}

pub struct CancelOutcome {
    pub refunded: bool,
}

// Admin ghi nhận buổi đã diễn ra: pick lot, debit, insert booking với status='attended'.
// Không check thời gian — dành cho trường hợp học viên tập rồi mới cập nhật vào app.
pub async fn admin_book_attended(
    pool: &PgPool,
    student_id: Uuid,
    session_id: Uuid,
    admin_id: Uuid,
) -> Result<Uuid, AppError> {
    let mut tx = pool.begin().await?;

    let session = queries::lock_session(&mut tx, session_id)
        .await?
        .ok_or(AppError::SessionNotFound)?;
    if session.booked_count >= session.capacity {
        return Err(AppError::SessionFull);
    }
    if queries::has_time_conflict(&mut tx, student_id, session_id).await? {
        let name = queries::get_student_name(&mut tx, student_id).await?;
        return Err(AppError::ScheduleConflictNamed(name));
    }

    let lot_id = queries::pick_credit_lot(
        &mut tx,
        student_id,
        session.class_type_id,
        session.branch_id,
        session.start_at,
    )
    .await?
    .ok_or(AppError::NoValidCredit)?;

    let booking_id = match queries::insert_booking_with_status(
        &mut tx,
        session_id,
        student_id,
        lot_id,
        admin_id,
        BookingChannel::Admin,
        "attended",
    )
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

    let (_, balance) = queries::adjust_lot(&mut tx, lot_id, -1).await?;
    queries::adjust_session_count(&mut tx, session_id, 1).await?;
    queries::write_ledger(
        &mut tx,
        lot_id,
        booking_id,
        -1,
        "admin_retroactive_book",
        admin_id,
        student_id,
        balance,
    )
    .await?;

    tx.commit().await?;
    Ok(booking_id)
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

    // 2. Kiểm tra trùng lịch học.
    if queries::has_time_conflict(&mut tx, student_id, session_id).await? {
        if channel == BookingChannel::Student {
            return Err(AppError::ScheduleConflict);
        }
        let name = queries::get_student_name(&mut tx, student_id).await?;
        return Err(AppError::ScheduleConflictNamed(name));
    }

    // 3. Khóa lot sẽ trừ. Không có lot hợp lệ = hết buổi, hết hạn, hoặc sai chi nhánh.
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

// Admin override: hủy bất kỳ booking nào (kể cả sát giờ/sau buổi học), có lý do bắt buộc.
// refund=true → hoàn +1 credit về lot cũ. Với attended/no_show không điều chỉnh slot session.
pub async fn admin_override_cancel(
    pool: &PgPool,
    booking_id: Uuid,
    admin_id: Uuid,
    refund: bool,
    reason: String,
) -> Result<CancelOutcome, AppError> {
    let mut tx = pool.begin().await?;

    let b = queries::lock_booking(&mut tx, booking_id)
        .await?
        .ok_or(AppError::BookingNotFound)?;
    if !matches!(b.status.as_str(), "booked" | "attended" | "no_show") {
        return Err(AppError::NotCancellable);
    }

    if refund {
        let (_, balance) = queries::adjust_lot(&mut tx, b.credit_lot_id, 1).await?;
        queries::write_ledger_meta(
            &mut tx,
            b.credit_lot_id,
            booking_id,
            1,
            "admin_override_refund",
            admin_id,
            b.student_id,
            balance,
            serde_json::json!({"admin_reason": reason}),
        )
        .await?;
    }
    if b.status == "booked" {
        queries::adjust_session_count(&mut tx, b.session_id, -1).await?;
    }
    queries::set_booking_cancelled_with_reason(&mut tx, booking_id, &reason).await?;
    tx.commit().await?;
    Ok(CancelOutcome { refunded: refund })
}

// Admin override: đổi lịch bất kể thời gian, reuse credit lot cũ để tránh trừ nhầm.
// Hoàn credit về lot cũ → trừ lại từ cùng lot cho session mới.
pub async fn admin_override_reschedule(
    pool: &PgPool,
    booking_id: Uuid,
    new_session_id: Uuid,
    admin_id: Uuid,
    reason: String,
) -> Result<Uuid, AppError> {
    let mut tx = pool.begin().await?;

    let b = queries::lock_booking(&mut tx, booking_id)
        .await?
        .ok_or(AppError::BookingNotFound)?;
    if b.status != "booked" {
        return Err(AppError::NotCancellable);
    }
    if b.session_id == new_session_id {
        return Err(AppError::InvalidInput("same_session"));
    }

    let new_session = queries::lock_session(&mut tx, new_session_id)
        .await?
        .ok_or(AppError::SessionNotFound)?;
    if new_session.status != "scheduled" {
        return Err(AppError::SessionNotBookable);
    }
    if new_session.booked_count >= new_session.capacity {
        return Err(AppError::SessionFull);
    }
    if queries::has_time_conflict(&mut tx, b.student_id, new_session_id).await? {
        let name = queries::get_student_name(&mut tx, b.student_id).await?;
        return Err(AppError::ScheduleConflictNamed(name));
    }

    // Hoàn về lot cũ rồi trừ lại cho session mới — cùng lot, net = 0.
    let (_, bal_after_refund) = queries::adjust_lot(&mut tx, b.credit_lot_id, 1).await?;
    queries::write_ledger_meta(
        &mut tx,
        b.credit_lot_id,
        booking_id,
        1,
        "admin_reschedule_refund",
        admin_id,
        b.student_id,
        bal_after_refund,
        serde_json::json!({"admin_reason": reason, "new_session_id": new_session_id}),
    )
    .await?;

    queries::set_booking_cancelled_with_reason(&mut tx, booking_id, &reason).await?;
    queries::adjust_session_count(&mut tx, b.session_id, -1).await?;

    let (_, bal_after_debit) = queries::adjust_lot(&mut tx, b.credit_lot_id, -1).await?;
    let new_booking_id = queries::insert_booking(
        &mut tx,
        new_session_id,
        b.student_id,
        b.credit_lot_id,
        admin_id,
        BookingChannel::Admin,
    )
    .await
    .map_err(|e| {
        if e.as_database_error().map(|d| d.is_unique_violation()) == Some(true) {
            AppError::AlreadyBooked
        } else {
            e.into()
        }
    })?;
    queries::adjust_session_count(&mut tx, new_session_id, 1).await?;
    queries::write_ledger_meta(
        &mut tx,
        b.credit_lot_id,
        new_booking_id,
        -1,
        "admin_reschedule_book",
        admin_id,
        b.student_id,
        bal_after_debit,
        serde_json::json!({"admin_reason": reason, "old_booking_id": booking_id}),
    )
    .await?;

    tx.commit().await?;
    Ok(new_booking_id)
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
