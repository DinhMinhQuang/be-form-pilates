// SQL nằm thẳng ở đây, gọi trên một &mut PgConnection (chính là transaction).
// Không bọc repository trait: app chỉ đánh đúng một Postgres, abstraction đó là tax thuần.
use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::{BookingChannel, BookingStatus};
use crate::error::AppError;

pub struct SessionRow {
    pub capacity: i32,
    pub booked_count: i32,
    pub status: String,
    pub class_type_id: Uuid,
    pub branch_id: Uuid,
    pub start_at: DateTime<Utc>,
}

pub struct BookingRow {
    pub session_id: Uuid,
    pub credit_lot_id: Uuid,
    pub status: String,
    pub start_at: DateTime<Utc>,
    pub student_id: Uuid,
}

// Khóa đúng dòng class_session (FOR UPDATE OF cs), join class_type chỉ để lấy category.
pub async fn lock_session(
    conn: &mut PgConnection,
    session_id: Uuid,
) -> Result<Option<SessionRow>, AppError> {
    let row: Option<(i32, i32, String, Uuid, Uuid, DateTime<Utc>)> = sqlx::query_as(
        r#"SELECT cs.capacity, cs.booked_count, cs.status, cs.class_type_id, cs.branch_id, cs.start_at
           FROM class_session cs
           WHERE cs.id = $1
           FOR UPDATE OF cs"#,
    )
    .bind(session_id)
    .fetch_optional(conn)
    .await?;

    match row {
        None => Ok(None),
        Some((capacity, booked_count, status, class_type_id, branch_id, start_at)) => Ok(Some(SessionRow {
            capacity,
            booked_count,
            status,
            class_type_id,
            branch_id,
            start_at,
        })),
    }
}

// Chọn lot còn hạn gần nhất (FIFO by expiry), khóa lại để serialize phần debit.
pub async fn pick_credit_lot(
    conn: &mut PgConnection,
    student_id: Uuid,
    class_type_id: Uuid,
    branch_id: Uuid,
    session_start_at: DateTime<Utc>,
) -> Result<Option<Uuid>, AppError> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT cl.id FROM credit_lot cl
           JOIN package_class_type pct ON pct.package_id = cl.package_id
           WHERE cl.student_id = $1
             AND pct.class_type_id = $2
             AND (cl.branch_id IS NULL OR cl.branch_id = $3)
             AND cl.sessions_remaining > 0
             AND cl.status = 'active'
             AND cl.activated_at <= now()
             AND cl.expires_at >= $4
           ORDER BY cl.expires_at ASC
           LIMIT 1
           FOR UPDATE OF cl"#,
    )
    .bind(student_id)
    .bind(class_type_id)
    .bind(branch_id)
    .bind(session_start_at)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|(id,)| id))
}

pub async fn insert_booking(
    conn: &mut PgConnection,
    session_id: Uuid,
    student_id: Uuid,
    lot_id: Uuid,
    booked_by: Uuid,
    channel: BookingChannel,
) -> Result<Uuid, sqlx::Error> {
    let row: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO booking
             (id, session_id, student_id, credit_lot_id, status, booked_by, channel, booked_at)
           VALUES (gen_random_uuid(), $1, $2, $3, 'booked', $4, $5, now())
           RETURNING id"#,
    )
    .bind(session_id)
    .bind(student_id)
    .bind(lot_id)
    .bind(booked_by)
    .bind(channel.as_str())
    .fetch_one(conn)
    .await?;
    Ok(row.0)
}

pub async fn adjust_lot(
    conn: &mut PgConnection,
    lot_id: Uuid,
    delta: i32,
) -> Result<(Uuid, i32), AppError> {
    let row: (Uuid, i32) = sqlx::query_as(
        "UPDATE credit_lot SET sessions_remaining = sessions_remaining + $2 WHERE id = $1 RETURNING student_id, sessions_remaining",
    )
        .bind(lot_id)
        .bind(delta)
        .fetch_one(conn)
        .await?;
    Ok(row)
}

pub async fn adjust_session_count(
    conn: &mut PgConnection,
    session_id: Uuid,
    delta: i32,
) -> Result<(), AppError> {
    sqlx::query("UPDATE class_session SET booked_count = booked_count + $2 WHERE id = $1")
        .bind(session_id)
        .bind(delta)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn write_ledger(
    conn: &mut PgConnection,
    lot_id: Uuid,
    booking_id: Uuid,
    delta: i32,
    reason: &str,
    actor_id: Uuid,
    student_id: Uuid,
    balance_after: i32,
) -> Result<(), AppError> {
    sqlx::query(
        r#"INSERT INTO credit_ledger
             (id, student_id, lot_id, booking_id, delta, balance_after, reason, actor_id, created_at)
           VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, now())"#,
    )
    .bind(student_id)
    .bind(lot_id)
    .bind(booking_id)
    .bind(delta)
    .bind(balance_after)
    .bind(reason)
    .bind(actor_id)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn lock_booking(
    conn: &mut PgConnection,
    booking_id: Uuid,
) -> Result<Option<BookingRow>, AppError> {
    let row: Option<(Uuid, Uuid, String, DateTime<Utc>, Uuid)> = sqlx::query_as(
        r#"SELECT b.session_id, b.credit_lot_id, b.status, cs.start_at, b.student_id
           FROM booking b
           JOIN class_session cs ON cs.id = b.session_id
           WHERE b.id = $1
           FOR UPDATE OF b"#,
    )
    .bind(booking_id)
    .fetch_optional(conn)
    .await?;

    Ok(row.map(
        |(session_id, credit_lot_id, status, start_at, student_id)| BookingRow {
            session_id,
            credit_lot_id,
            status,
            start_at,
            student_id,
        },
    ))
}

pub async fn set_booking_status(
    conn: &mut PgConnection,
    booking_id: Uuid,
    status: BookingStatus,
) -> Result<(), AppError> {
    sqlx::query("UPDATE booking SET status = $2, cancelled_at = now() WHERE id = $1")
        .bind(booking_id)
        .bind(status.as_str())
        .execute(conn)
        .await?;
    Ok(())
}
