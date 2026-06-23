use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{AuthStudent, AuthUser},
    error::AppError,
    state::AppState,
};

#[derive(Serialize)]
pub struct MeResponse {
    id: Uuid,
    role: String,
    full_name: String,
    email: Option<String>,
    phone: Option<String>,
}

pub async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<MeResponse>, AppError> {
    let row: (String, String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT role, full_name, email, phone FROM app_user WHERE id = $1")
            .bind(user.id)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(MeResponse {
        id: user.id,
        role: row.0,
        full_name: row.1,
        email: row.2,
        phone: row.3,
    }))
}

#[derive(Deserialize)]
pub struct ScheduleQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    branch_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct SessionView {
    id: Uuid,
    branch_id: Uuid,
    branch_name: String,
    class_name: String,
    category: String,
    trainer_name: Option<String>,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    capacity: i32,
    available_slots: i32,
}

pub async fn sessions(
    State(state): State<AppState>,
    _student: AuthStudent,
    Query(query): Query<ScheduleQuery>,
) -> Result<Json<Vec<SessionView>>, AppError> {
    let from = query.from.unwrap_or_else(Utc::now);
    let to = query.to.unwrap_or(from + Duration::days(31));
    if to <= from || to - from > Duration::days(93) {
        return Err(AppError::InvalidInput("invalid_schedule_range"));
    }

    let rows: Vec<(
        Uuid,
        Uuid,
        String,
        String,
        String,
        Option<String>,
        DateTime<Utc>,
        DateTime<Utc>,
        i32,
        i32,
    )> = sqlx::query_as(
        r#"SELECT cs.id, b.id, b.name, ct.name, ct.category, trainer.full_name,
                      cs.start_at, cs.end_at, cs.capacity, cs.capacity - cs.booked_count
               FROM class_session cs
               JOIN branch b ON b.id = cs.branch_id
               JOIN class_type ct ON ct.id = cs.class_type_id
               LEFT JOIN app_user trainer ON trainer.id = cs.trainer_id
               WHERE cs.status = 'scheduled' AND cs.start_at >= $1 AND cs.start_at < $2
                 AND ($3::uuid IS NULL OR cs.branch_id = $3)
               ORDER BY cs.start_at"#,
    )
    .bind(from)
    .bind(to)
    .bind(query.branch_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| SessionView {
                id: row.0,
                branch_id: row.1,
                branch_name: row.2,
                class_name: row.3,
                category: row.4,
                trainer_name: row.5,
                start_at: row.6,
                end_at: row.7,
                capacity: row.8,
                available_slots: row.9,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct BookingView {
    id: Uuid,
    session_id: Uuid,
    class_name: String,
    branch_name: String,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    status: String,
    booked_at: DateTime<Utc>,
    cancellable: bool,
}

pub async fn bookings(
    State(state): State<AppState>,
    student: AuthStudent,
) -> Result<Json<Vec<BookingView>>, AppError> {
    let rows: Vec<(
        Uuid,
        Uuid,
        String,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        String,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"SELECT bk.id, cs.id, ct.name, br.name, cs.start_at, cs.end_at, bk.status, bk.booked_at
               FROM booking bk
               JOIN class_session cs ON cs.id = bk.session_id
               JOIN class_type ct ON ct.id = cs.class_type_id
               JOIN branch br ON br.id = cs.branch_id
               WHERE bk.student_id = $1
               ORDER BY cs.start_at DESC LIMIT 200"#,
    )
    .bind(student.0)
    .fetch_all(&state.pool)
    .await?;
    let cancellation_deadline = Utc::now() + Duration::hours(6);
    Ok(Json(
        rows.into_iter()
            .map(|row| BookingView {
                id: row.0,
                session_id: row.1,
                class_name: row.2,
                branch_name: row.3,
                start_at: row.4,
                end_at: row.5,
                status: row.6.clone(),
                booked_at: row.7,
                cancellable: row.6 == "booked" && row.4 >= cancellation_deadline,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct CreditView {
    lot_id: Uuid,
    package_name: String,
    sessions_total: i32,
    sessions_remaining: i32,
    activated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    status: String,
}

pub async fn credits(
    State(state): State<AppState>,
    student: AuthStudent,
) -> Result<Json<Vec<CreditView>>, AppError> {
    let rows: Vec<(Uuid, String, i32, i32, DateTime<Utc>, DateTime<Utc>, String)> = sqlx::query_as(
        r#"SELECT cl.id, cp.name, cl.sessions_total, cl.sessions_remaining,
                  cl.activated_at, cl.expires_at, cl.status
           FROM credit_lot cl JOIN course_package cp ON cp.id = cl.package_id
           WHERE cl.student_id = $1 ORDER BY cl.expires_at"#,
    )
    .bind(student.0)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| CreditView {
                lot_id: row.0,
                package_name: row.1,
                sessions_total: row.2,
                sessions_remaining: row.3,
                activated_at: row.4,
                expires_at: row.5,
                status: row.6,
            })
            .collect(),
    ))
}
