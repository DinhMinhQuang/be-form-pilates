use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{AuthAdmin, normalize_phone},
    booking::service,
    domain::BookingChannel,
    error::AppError,
    state::AppState,
};

#[derive(Deserialize)]
pub struct CreateSessionInput {
    branch_id: Uuid,
    class_type_id: Uuid,
    trainer_id: Uuid,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    capacity: Option<i32>,
}

pub async fn create_session(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Json(input): Json<CreateSessionInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if input.end_at <= input.start_at || input.start_at <= Utc::now() {
        return Err(AppError::InvalidInput("invalid_session_time"));
    }
    let config: Option<(i32,)> = sqlx::query_as(
        r#"SELECT ct.default_capacity
           FROM branch_class_type bct JOIN class_type ct ON ct.id = bct.class_type_id
           WHERE bct.branch_id = $1 AND bct.class_type_id = $2 AND bct.enabled"#,
    )
    .bind(input.branch_id)
    .bind(input.class_type_id)
    .fetch_optional(&state.pool)
    .await?;
    let (default_capacity,) =
        config.ok_or(AppError::InvalidInput("class_not_available_at_branch"))?;
    let capacity = input.capacity.unwrap_or(default_capacity);
    if !(1..=6).contains(&capacity) {
        return Err(AppError::InvalidInput("invalid_capacity"));
    }
    let trainer_ok: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM app_user WHERE id = $1 AND role = 'trainer' AND status = 'active')",
    ).bind(input.trainer_id).fetch_one(&state.pool).await?;
    if !trainer_ok.0 {
        return Err(AppError::InvalidInput("invalid_trainer"));
    }
    let overlap: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(SELECT 1 FROM class_session WHERE trainer_id = $1 AND status = 'scheduled'
           AND start_at < $3 AND end_at > $2)"#,
    )
    .bind(input.trainer_id)
    .bind(input.start_at)
    .bind(input.end_at)
    .fetch_one(&state.pool)
    .await?;
    if overlap.0 {
        return Err(AppError::Conflict);
    }
    let (id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO class_session
             (branch_id, class_type_id, trainer_id, start_at, end_at, capacity, created_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"#,
    )
    .bind(input.branch_id)
    .bind(input.class_type_id)
    .bind(input.trainer_id)
    .bind(input.start_at)
    .bind(input.end_at)
    .bind(capacity)
    .bind(admin.0)
    .fetch_one(&state.pool)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"session_id": id})),
    ))
}

#[derive(Deserialize)]
pub struct SessionQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    branch_id: Option<Uuid>,
    trainer_id: Option<Uuid>,
    status: Option<String>,
}

#[derive(Serialize)]
pub struct AdminSessionView {
    id: Uuid,
    branch_id: Uuid,
    branch_name: String,
    class_type_id: Uuid,
    class_name: String,
    trainer_id: Option<Uuid>,
    trainer_name: Option<String>,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    capacity: i32,
    booked_count: i32,
    status: String,
}

pub async fn sessions(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Vec<AdminSessionView>>, AppError> {
    let from = query.from.unwrap_or_else(|| Utc::now() - Duration::days(7));
    let to = query.to.unwrap_or(from + Duration::days(62));
    if to <= from || to - from > Duration::days(190) {
        return Err(AppError::InvalidInput("invalid_schedule_range"));
    }
    if query
        .status
        .as_deref()
        .is_some_and(|v| !matches!(v, "scheduled" | "cancelled" | "completed"))
    {
        return Err(AppError::InvalidInput("invalid_session_status"));
    }

    let rows: Vec<(
        Uuid,
        Uuid,
        String,
        Uuid,
        String,
        Option<Uuid>,
        Option<String>,
        DateTime<Utc>,
        DateTime<Utc>,
        i32,
        i32,
        String,
    )> = sqlx::query_as(
        r#"SELECT cs.id, br.id, br.name, ct.id, ct.name, trainer.id, trainer.full_name,
                  cs.start_at, cs.end_at, cs.capacity, cs.booked_count, cs.status
           FROM class_session cs
           JOIN branch br ON br.id = cs.branch_id
           JOIN class_type ct ON ct.id = cs.class_type_id
           LEFT JOIN app_user trainer ON trainer.id = cs.trainer_id
           WHERE cs.start_at >= $1 AND cs.start_at < $2
             AND ($3::uuid IS NULL OR cs.branch_id = $3)
             AND ($4::uuid IS NULL OR cs.trainer_id = $4)
             AND ($5::text IS NULL OR cs.status = $5)
           ORDER BY cs.start_at DESC
           LIMIT 500"#,
    )
    .bind(from)
    .bind(to)
    .bind(query.branch_id)
    .bind(query.trainer_id)
    .bind(query.status)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| AdminSessionView {
                id: r.0,
                branch_id: r.1,
                branch_name: r.2,
                class_type_id: r.3,
                class_name: r.4,
                trainer_id: r.5,
                trainer_name: r.6,
                start_at: r.7,
                end_at: r.8,
                capacity: r.9,
                booked_count: r.10,
                status: r.11,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct UpdateSessionInput {
    branch_id: Option<Uuid>,
    class_type_id: Option<Uuid>,
    trainer_id: Option<Uuid>,
    start_at: Option<DateTime<Utc>>,
    end_at: Option<DateTime<Utc>>,
    capacity: Option<i32>,
    status: Option<String>,
}

pub async fn update_session(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateSessionInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if input
        .status
        .as_deref()
        .is_some_and(|v| !matches!(v, "scheduled" | "cancelled" | "completed"))
    {
        return Err(AppError::InvalidInput("invalid_session_status"));
    }
    let current: Option<(Uuid, Uuid, Option<Uuid>, DateTime<Utc>, DateTime<Utc>, i32, i32)> =
        sqlx::query_as(
            "SELECT branch_id, class_type_id, trainer_id, start_at, end_at, capacity, booked_count FROM class_session WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let (branch_id, class_type_id, trainer_id, start_at, end_at, capacity, booked_count) =
        current.ok_or(AppError::SessionNotFound)?;

    let next_branch = input.branch_id.unwrap_or(branch_id);
    let next_class_type = input.class_type_id.unwrap_or(class_type_id);
    let next_trainer = input.trainer_id.or(trainer_id);
    let next_start = input.start_at.unwrap_or(start_at);
    let next_end = input.end_at.unwrap_or(end_at);
    let next_capacity = input.capacity.unwrap_or(capacity);
    if next_end <= next_start || next_capacity < booked_count || !(1..=6).contains(&next_capacity) {
        return Err(AppError::InvalidInput("invalid_session_update"));
    }
    let available: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS(SELECT 1 FROM branch_class_type
           WHERE branch_id = $1 AND class_type_id = $2 AND enabled)"#,
    )
    .bind(next_branch)
    .bind(next_class_type)
    .fetch_one(&state.pool)
    .await?;
    if !available.0 {
        return Err(AppError::InvalidInput("class_not_available_at_branch"));
    }
    if let Some(trainer_id) = next_trainer {
        let trainer_ok: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM app_user WHERE id = $1 AND role = 'trainer' AND status = 'active')",
        )
        .bind(trainer_id)
        .fetch_one(&state.pool)
        .await?;
        if !trainer_ok.0 {
            return Err(AppError::InvalidInput("invalid_trainer"));
        }
        let overlap: (bool,) = sqlx::query_as(
            r#"SELECT EXISTS(SELECT 1 FROM class_session
               WHERE id <> $1 AND trainer_id = $2 AND status = 'scheduled'
                 AND start_at < $4 AND end_at > $3)"#,
        )
        .bind(id)
        .bind(trainer_id)
        .bind(next_start)
        .bind(next_end)
        .fetch_one(&state.pool)
        .await?;
        if overlap.0 {
            return Err(AppError::Conflict);
        }
    }

    let result = sqlx::query(
        r#"UPDATE class_session
           SET branch_id = $2, class_type_id = $3, trainer_id = $4,
               start_at = $5, end_at = $6, capacity = $7,
               status = COALESCE($8, status)
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(next_branch)
    .bind(next_class_type)
    .bind(next_trainer)
    .bind(next_start)
    .bind(next_end)
    .bind(next_capacity)
    .bind(input.status)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::SessionNotFound);
    }
    Ok(Json(serde_json::json!({"updated": true})))
}

pub async fn cancel_session(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query(
        "UPDATE class_session SET status = 'cancelled', booked_count = 0 WHERE id = $1 AND status = 'scheduled'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::SessionNotFound);
    }

    let bookings: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, student_id, credit_lot_id FROM booking WHERE session_id = $1 AND status = 'booked' FOR UPDATE",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for (booking_id, student_id, lot_id) in &bookings {
        let (balance_after,): (i32,) = sqlx::query_as(
            "UPDATE credit_lot SET sessions_remaining = sessions_remaining + 1 WHERE id = $1 RETURNING sessions_remaining",
        )
        .bind(lot_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO credit_ledger
               (student_id, lot_id, booking_id, delta, balance_after, reason, actor_id)
               VALUES ($1, $2, $3, 1, $4, 'session_cancelled_refund', $5)"#,
        )
        .bind(student_id)
        .bind(lot_id)
        .bind(booking_id)
        .bind(balance_after)
        .bind(admin.0)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE booking SET status = 'cancelled_refunded', cancelled_at = now(), cancellation_reason = 'session_cancelled' WHERE session_id = $1 AND status = 'booked'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({"cancelled": true, "refunded_bookings": bookings.len()}),
    ))
}

#[derive(Deserialize)]
pub struct CreateTrainerInput {
    full_name: String,
    email: String,
    phone: Option<String>,
    password: String,
}

#[derive(Deserialize)]
pub struct StaffQuery {
    status: Option<String>,
}

#[derive(Serialize)]
pub struct TrainerView {
    id: Uuid,
    full_name: String,
    email: Option<String>,
    phone: Option<String>,
    status: String,
    last_login_at: Option<DateTime<Utc>>,
}

pub async fn trainers(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Query(query): Query<StaffQuery>,
) -> Result<Json<Vec<TrainerView>>, AppError> {
    if query
        .status
        .as_deref()
        .is_some_and(|v| v != "active" && v != "disabled")
    {
        return Err(AppError::InvalidInput("invalid_status"));
    }
    let rows: Vec<(
        Uuid,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        r#"SELECT u.id, u.full_name, u.email, u.phone, u.status, sc.last_login_at
               FROM app_user u
               LEFT JOIN staff_credential sc ON sc.user_id = u.id
               WHERE u.role = 'trainer'
                 AND ($1::text IS NULL OR u.status = $1)
               ORDER BY u.full_name
               LIMIT 500"#,
    )
    .bind(query.status)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| TrainerView {
                id: r.0,
                full_name: r.1,
                email: r.2,
                phone: r.3,
                status: r.4,
                last_login_at: r.5,
            })
            .collect(),
    ))
}

pub async fn create_trainer(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Json(input): Json<CreateTrainerInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if input.password.len() < 10 {
        return Err(AppError::InvalidInput("password_too_short"));
    }
    let hash = hash_password(&input.password)?;
    let mut tx = state.pool.begin().await?;
    let user = sqlx::query_as::<_, (Uuid,)>(
        "INSERT INTO app_user (role, email, phone, full_name) VALUES ('trainer', lower($1), $2, $3) RETURNING id",
    ).bind(input.email.trim()).bind(input.phone.map(|v| normalize_phone(&v))).bind(input.full_name.trim())
        .fetch_one(&mut *tx).await.map_err(map_unique)?;
    sqlx::query("INSERT INTO staff_credential (user_id, password_hash) VALUES ($1, $2)")
        .bind(user.0)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"trainer_id": user.0})),
    ))
}

#[derive(Deserialize)]
pub struct UpdateTrainerInput {
    full_name: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    password: Option<String>,
    status: Option<String>,
}

pub async fn update_trainer(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTrainerInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if input
        .status
        .as_deref()
        .is_some_and(|v| v != "active" && v != "disabled")
    {
        return Err(AppError::InvalidInput("invalid_status"));
    }
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query(
        r#"UPDATE app_user SET full_name = COALESCE($2, full_name), email = COALESCE(lower($3), email),
           phone = COALESCE($4, phone), status = COALESCE($5, status), updated_at = now()
           WHERE id = $1 AND role = 'trainer'"#,
    ).bind(id).bind(input.full_name).bind(input.email)
        .bind(input.phone.map(|v| normalize_phone(&v))).bind(input.status)
        .execute(&mut *tx).await.map_err(map_unique)?;
    if result.rows_affected() == 0 {
        return Err(AppError::BookingNotFound);
    }
    if let Some(password) = input.password {
        if password.len() < 10 {
            return Err(AppError::InvalidInput("password_too_short"));
        }
        sqlx::query("UPDATE staff_credential SET password_hash = $2 WHERE user_id = $1")
            .bind(id)
            .bind(hash_password(&password)?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({"updated": true})))
}

pub async fn delete_trainer(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let result = sqlx::query("UPDATE app_user SET status = 'disabled', updated_at = now() WHERE id = $1 AND role = 'trainer'")
        .bind(id).execute(&state.pool).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::BookingNotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct StudentView {
    id: Uuid,
    full_name: String,
    email: Option<String>,
    phone: Option<String>,
    status: String,
    credits: i64,
}

pub async fn students(
    State(state): State<AppState>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<StudentView>>, AppError> {
    let rows: Vec<(Uuid, String, Option<String>, Option<String>, String, i64)> = sqlx::query_as(
        r#"SELECT u.id, u.full_name, u.email, u.phone, u.status, COALESCE(SUM(cl.sessions_remaining), 0)::bigint
           FROM app_user u LEFT JOIN credit_lot cl ON cl.student_id = u.id AND cl.status = 'active'
           WHERE u.role = 'student' GROUP BY u.id ORDER BY u.full_name LIMIT 500"#,
    ).fetch_all(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| StudentView {
                id: r.0,
                full_name: r.1,
                email: r.2,
                phone: r.3,
                status: r.4,
                credits: r.5,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct CreateStudentInput {
    full_name: String,
    email: Option<String>,
    phone: Option<String>,
    notes: Option<String>,
}

pub async fn create_student(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Json(input): Json<CreateStudentInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let email = input
        .email
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty());
    let phone = input
        .phone
        .map(|v| normalize_phone(&v))
        .filter(|v| !v.is_empty());
    if email.is_none() && phone.is_none() {
        return Err(AppError::InvalidInput("email_or_phone_required"));
    }
    let mut tx = state.pool.begin().await?;
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO app_user (role, email, phone, full_name) VALUES ('student', $1, $2, $3) RETURNING id",
    )
    .bind(email)
    .bind(phone)
    .bind(input.full_name.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(map_unique)?;
    sqlx::query("INSERT INTO student_profile (user_id, notes) VALUES ($1, $2)")
        .bind(id)
        .bind(input.notes)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"student_id": id})),
    ))
}

#[derive(Serialize)]
pub struct CreditLotView {
    id: Uuid,
    package_id: Uuid,
    package_name: String,
    sessions_total: i32,
    sessions_remaining: i32,
    activated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    status: String,
    branch_id: Option<Uuid>,
    branch_name: Option<String>,
}

#[derive(Serialize)]
pub struct StudentDetail {
    id: Uuid,
    full_name: String,
    email: Option<String>,
    phone: Option<String>,
    status: String,
    notes: Option<String>,
    credits: i64,
    credit_lots: Vec<CreditLotView>,
}

pub async fn student_detail(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<Json<StudentDetail>, AppError> {
    let row: Option<(
        Uuid,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
    )> = sqlx::query_as(
        r#"SELECT u.id, u.full_name, u.email, u.phone, u.status, sp.notes,
                      COALESCE(SUM(cl.sessions_remaining), 0)::bigint
               FROM app_user u
               LEFT JOIN student_profile sp ON sp.user_id = u.id
               LEFT JOIN credit_lot cl ON cl.student_id = u.id AND cl.status = 'active'
               WHERE u.id = $1 AND u.role = 'student'
               GROUP BY u.id, sp.notes"#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let r = row.ok_or(AppError::BookingNotFound)?;

    let lot_rows: Vec<(
        Uuid,
        Uuid,
        String,
        i32,
        i32,
        DateTime<Utc>,
        DateTime<Utc>,
        String,
        Option<Uuid>,
        Option<String>,
    )> = sqlx::query_as(
        r#"SELECT cl.id, cp.id, cp.name, cl.sessions_total, cl.sessions_remaining,
                  cl.activated_at, cl.expires_at, cl.status, cl.branch_id, br.name
           FROM credit_lot cl
           JOIN course_package cp ON cp.id = cl.package_id
           LEFT JOIN branch br ON br.id = cl.branch_id
           WHERE cl.student_id = $1
           ORDER BY cl.expires_at"#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(StudentDetail {
        id: r.0,
        full_name: r.1,
        email: r.2,
        phone: r.3,
        status: r.4,
        notes: r.5,
        credits: r.6,
        credit_lots: lot_rows
            .into_iter()
            .map(|l| CreditLotView {
                id: l.0,
                package_id: l.1,
                package_name: l.2,
                sessions_total: l.3,
                sessions_remaining: l.4,
                activated_at: l.5,
                expires_at: l.6,
                status: l.7,
                branch_id: l.8,
                branch_name: l.9,
            })
            .collect(),
    }))
}

#[derive(Deserialize)]
pub struct UpdateStudentInput {
    full_name: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    status: Option<String>,
    notes: Option<String>,
}

pub async fn update_student(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateStudentInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if input
        .status
        .as_deref()
        .is_some_and(|v| v != "active" && v != "disabled")
    {
        return Err(AppError::InvalidInput("invalid_status"));
    }
    let mut tx = state.pool.begin().await?;
    let result = sqlx::query(
        r#"UPDATE app_user
           SET full_name = COALESCE($2, full_name),
               email = COALESCE(lower($3), email),
               phone = COALESCE($4, phone),
               status = COALESCE($5, status),
               updated_at = now()
           WHERE id = $1 AND role = 'student'"#,
    )
    .bind(id)
    .bind(input.full_name.map(|v| v.trim().to_owned()))
    .bind(input.email.map(|v| v.trim().to_lowercase()))
    .bind(input.phone.map(|v| normalize_phone(&v)))
    .bind(input.status)
    .execute(&mut *tx)
    .await
    .map_err(map_unique)?;
    if result.rows_affected() == 0 {
        return Err(AppError::BookingNotFound);
    }
    if let Some(notes) = input.notes {
        sqlx::query(
            r#"INSERT INTO student_profile (user_id, notes) VALUES ($1, $2)
               ON CONFLICT (user_id) DO UPDATE SET notes = EXCLUDED.notes"#,
        )
        .bind(id)
        .bind(notes)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({"updated": true})))
}

#[derive(Deserialize)]
pub struct BookingQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    student_id: Option<Uuid>,
    session_id: Option<Uuid>,
    status: Option<String>,
}

#[derive(Serialize)]
pub struct AdminBookingView {
    id: Uuid,
    session_id: Uuid,
    student_id: Uuid,
    student_name: String,
    student_phone: Option<String>,
    branch_name: String,
    class_name: String,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    status: String,
    channel: String,
    booked_at: DateTime<Utc>,
}

pub async fn bookings(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Query(query): Query<BookingQuery>,
) -> Result<Json<Vec<AdminBookingView>>, AppError> {
    let from = query
        .from
        .unwrap_or_else(|| Utc::now() - Duration::days(31));
    let to = query.to.unwrap_or(from + Duration::days(93));
    if to <= from || to - from > Duration::days(190) {
        return Err(AppError::InvalidInput("invalid_booking_range"));
    }
    let rows: Vec<(
        Uuid,
        Uuid,
        Uuid,
        String,
        Option<String>,
        String,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        String,
        String,
        DateTime<Utc>,
    )> = sqlx::query_as(
        r#"SELECT bk.id, cs.id, u.id, u.full_name, u.phone, br.name, ct.name,
                  cs.start_at, cs.end_at, bk.status, bk.channel, bk.booked_at
           FROM booking bk
           JOIN app_user u ON u.id = bk.student_id
           JOIN class_session cs ON cs.id = bk.session_id
           JOIN branch br ON br.id = cs.branch_id
           JOIN class_type ct ON ct.id = cs.class_type_id
           WHERE cs.start_at >= $1 AND cs.start_at < $2
             AND ($3::uuid IS NULL OR bk.student_id = $3)
             AND ($4::uuid IS NULL OR bk.session_id = $4)
             AND ($5::text IS NULL OR bk.status = $5)
           ORDER BY cs.start_at DESC, u.full_name
           LIMIT 1000"#,
    )
    .bind(from)
    .bind(to)
    .bind(query.student_id)
    .bind(query.session_id)
    .bind(query.status)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| AdminBookingView {
                id: r.0,
                session_id: r.1,
                student_id: r.2,
                student_name: r.3,
                student_phone: r.4,
                branch_name: r.5,
                class_name: r.6,
                start_at: r.7,
                end_at: r.8,
                status: r.9,
                channel: r.10,
                booked_at: r.11,
            })
            .collect(),
    ))
}

pub async fn book_for_student(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path((student_id, session_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let id = service::book_class(
        &state.pool,
        student_id,
        session_id,
        admin.0,
        BookingChannel::Admin,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"booking_id": id})),
    ))
}

pub async fn cancel_for_student(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    service::cancel_booking(&state.pool, id, admin.0, true).await?;
    Ok(Json(serde_json::json!({"refunded": true})))
}

pub async fn delete_student(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let result = sqlx::query(
        "UPDATE app_user SET status = 'disabled', updated_at = now() WHERE id = $1 AND role = 'student'",
    )
    .bind(id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::BookingNotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CreateCreditLotInput {
    package_id: Uuid,
    sessions_total: i32,
    expires_at: DateTime<Utc>,
    branch_id: Option<Uuid>,
}

pub async fn create_credit_lot(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path(student_id): Path<Uuid>,
    Json(input): Json<CreateCreditLotInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if input.sessions_total <= 0 {
        return Err(AppError::InvalidInput("invalid_sessions_total"));
    }
    if input.expires_at <= Utc::now() {
        return Err(AppError::InvalidInput("invalid_expiry"));
    }
    let student_ok: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM app_user WHERE id = $1 AND role = 'student')")
            .bind(student_id)
            .fetch_one(&state.pool)
            .await?;
    if !student_ok.0 {
        return Err(AppError::BookingNotFound);
    }
    let package_ok: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM course_package WHERE id = $1)")
            .bind(input.package_id)
            .fetch_one(&state.pool)
            .await?;
    if !package_ok.0 {
        return Err(AppError::InvalidInput("invalid_package"));
    }

    let mut tx = state.pool.begin().await?;
    let (lot_id,): (Uuid,) = sqlx::query_as(
        r#"INSERT INTO credit_lot
             (student_id, package_id, sessions_total, sessions_remaining, activated_at, expires_at, status, branch_id)
           VALUES ($1, $2, $3, $3, now(), $4, 'active', $5) RETURNING id"#,
    )
    .bind(student_id)
    .bind(input.package_id)
    .bind(input.sessions_total)
    .bind(input.expires_at)
    .bind(input.branch_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        r#"INSERT INTO credit_ledger (student_id, lot_id, delta, balance_after, reason, actor_id)
           VALUES ($1, $2, $3, $3, 'admin_manual_grant', $4)"#,
    )
    .bind(student_id)
    .bind(lot_id)
    .bind(input.sessions_total)
    .bind(admin.0)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"lot_id": lot_id})),
    ))
}

#[derive(Deserialize)]
pub struct AdjustCreditInput {
    delta: Option<i32>,
    expires_at: Option<DateTime<Utc>>,
    reason: String,
}

pub async fn adjust_credit(
    State(state): State<AppState>,
    admin: AuthAdmin,
    Path((student_id, lot_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<AdjustCreditInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if input.reason.trim().is_empty() || (input.delta.is_none() && input.expires_at.is_none()) {
        return Err(AppError::InvalidInput("adjustment_and_reason_required"));
    }
    let mut tx = state.pool.begin().await?;
    let lot: Option<(i32, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT sessions_total, sessions_remaining, expires_at FROM credit_lot WHERE id = $1 AND student_id = $2 FOR UPDATE",
    ).bind(lot_id).bind(student_id).fetch_optional(&mut *tx).await?;
    let (mut total, mut remaining, old_expiry) = lot.ok_or(AppError::BookingNotFound)?;
    if let Some(delta) = input.delta {
        remaining = remaining
            .checked_add(delta)
            .ok_or(AppError::InvalidInput("invalid_credit_delta"))?;
        if remaining < 0 {
            return Err(AppError::InvalidInput("insufficient_credit"));
        }
        total = total.max(remaining);
        sqlx::query(
            "UPDATE credit_lot SET sessions_total = $2, sessions_remaining = $3 WHERE id = $1",
        )
        .bind(lot_id)
        .bind(total)
        .bind(remaining)
        .execute(&mut *tx)
        .await?;
        if delta != 0 {
            sqlx::query(
                r#"INSERT INTO credit_ledger (student_id, lot_id, delta, balance_after, reason, actor_id, metadata)
                   VALUES ($1, $2, $3, $4, 'admin_adjustment', $5, jsonb_build_object('reason', $6::text))"#,
            ).bind(student_id).bind(lot_id).bind(delta).bind(remaining).bind(admin.0).bind(&input.reason)
                .execute(&mut *tx).await?;
        }
    }
    if let Some(new_expiry) = input.expires_at {
        sqlx::query("UPDATE credit_lot SET expires_at = $2 WHERE id = $1")
            .bind(lot_id)
            .bind(new_expiry)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO credit_expiry_change (credit_lot_id, old_expires_at, new_expires_at, reason, changed_by) VALUES ($1, $2, $3, $4, $5)",
        ).bind(lot_id).bind(old_expiry).bind(new_expiry).bind(&input.reason).bind(admin.0)
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({"sessions_remaining": remaining})))
}

#[derive(Deserialize)]
pub struct ProductMappingInput {
    haravan_product_id: Option<String>,
    haravan_variant_id: String,
    package_id: Uuid,
    branch_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct ProductMappingView {
    id: Uuid,
    haravan_product_id: Option<String>,
    haravan_variant_id: String,
    package_id: Uuid,
    package_name: String,
    branch_id: Option<Uuid>,
    branch_name: Option<String>,
    active: bool,
}

pub async fn product_mappings(
    State(state): State<AppState>,
    _admin: AuthAdmin,
) -> Result<Json<Vec<ProductMappingView>>, AppError> {
    let rows: Vec<(
        Uuid,
        Option<String>,
        String,
        Uuid,
        String,
        Option<Uuid>,
        Option<String>,
        bool,
    )> = sqlx::query_as(
        r#"SELECT hpm.id, hpm.haravan_product_id, hpm.haravan_variant_id,
                      cp.id, cp.name, br.id, br.name, hpm.active
               FROM haravan_product_mapping hpm
               JOIN course_package cp ON cp.id = hpm.package_id
               LEFT JOIN branch br ON br.id = hpm.branch_id
               ORDER BY hpm.active DESC, cp.sessions, hpm.haravan_variant_id"#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ProductMappingView {
                id: r.0,
                haravan_product_id: r.1,
                haravan_variant_id: r.2,
                package_id: r.3,
                package_name: r.4,
                branch_id: r.5,
                branch_name: r.6,
                active: r.7,
            })
            .collect(),
    ))
}

pub async fn create_product_mapping(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Json(input): Json<ProductMappingInput>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO haravan_product_mapping (haravan_product_id, haravan_variant_id, package_id, branch_id) VALUES ($1, $2, $3, $4) RETURNING id",
    ).bind(input.haravan_product_id).bind(input.haravan_variant_id).bind(input.package_id).bind(input.branch_id)
        .fetch_one(&state.pool).await.map_err(map_unique)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"mapping_id": id})),
    ))
}

#[derive(Deserialize)]
pub struct UpdateProductMappingInput {
    haravan_product_id: Option<String>,
    haravan_variant_id: Option<String>,
    package_id: Option<Uuid>,
    branch_id: Option<Uuid>,
    active: Option<bool>,
}

pub async fn update_product_mapping(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateProductMappingInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query(
        r#"UPDATE haravan_product_mapping
           SET haravan_product_id = COALESCE($2, haravan_product_id),
               haravan_variant_id = COALESCE($3, haravan_variant_id),
               package_id = COALESCE($4, package_id),
               branch_id = COALESCE($5, branch_id),
               active = COALESCE($6, active)
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(input.haravan_product_id)
    .bind(input.haravan_variant_id)
    .bind(input.package_id)
    .bind(input.branch_id)
    .bind(input.active)
    .execute(&state.pool)
    .await
    .map_err(map_unique)?;
    if result.rows_affected() == 0 {
        return Err(AppError::BookingNotFound);
    }
    Ok(Json(serde_json::json!({"updated": true})))
}

#[derive(Deserialize)]
pub struct PackageClassTypesInput {
    class_type_ids: Vec<Uuid>,
}

pub async fn set_package_class_types(
    State(state): State<AppState>,
    _admin: AuthAdmin,
    Path(package_id): Path<Uuid>,
    Json(input): Json<PackageClassTypesInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM package_class_type WHERE package_id = $1")
        .bind(package_id)
        .execute(&mut *tx)
        .await?;
    for class_type_id in input.class_type_ids {
        sqlx::query("INSERT INTO package_class_type (package_id, class_type_id) VALUES ($1, $2)")
            .bind(package_id)
            .bind(class_type_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({"updated": true})))
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::Integration("password_hash_failed"))
}

fn map_unique(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        AppError::Conflict
    } else {
        AppError::Db(error)
    }
}
