use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use axum::http::StatusCode;

use crate::{
    auth::AuthTrainer, booking::service as booking_service, domain::BookingChannel,
    error::AppError, state::AppState,
};

#[derive(Deserialize)]
pub struct RangeQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct TrainerSession {
    id: Uuid,
    class_name: String,
    branch_name: String,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    booked_count: i32,
    capacity: i32,
    status: String,
}

pub async fn sessions(
    State(state): State<AppState>,
    trainer: AuthTrainer,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<TrainerSession>>, AppError> {
    let from = query.from.unwrap_or_else(Utc::now);
    let to = query.to.unwrap_or(from + Duration::days(31));
    if to <= from || to - from > Duration::days(93) {
        return Err(AppError::InvalidInput("invalid_schedule_range"));
    }
    let rows: Vec<(Uuid, String, String, DateTime<Utc>, DateTime<Utc>, i32, i32, String)> = sqlx::query_as(
        r#"SELECT cs.id, ct.name, br.name, cs.start_at, cs.end_at, cs.booked_count, cs.capacity, cs.status
           FROM class_session cs JOIN class_type ct ON ct.id = cs.class_type_id
           JOIN branch br ON br.id = cs.branch_id
           JOIN app_user actor ON actor.id = $1
           WHERE (cs.trainer_id = $1 OR actor.role = 'admin')
             AND cs.start_at >= $2 AND cs.start_at < $3 ORDER BY cs.start_at"#,
    )
    .bind(trainer.0)
    .bind(from)
    .bind(to)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| TrainerSession {
                id: r.0,
                class_name: r.1,
                branch_name: r.2,
                start_at: r.3,
                end_at: r.4,
                booked_count: r.5,
                capacity: r.6,
                status: r.7,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct StudentInClass {
    booking_id: Uuid,
    student_id: Uuid,
    full_name: String,
    phone: Option<String>,
    status: String,
}

pub async fn students(
    State(state): State<AppState>,
    trainer: AuthTrainer,
    Path(session_id): Path<Uuid>,
) -> Result<Json<Vec<StudentInClass>>, AppError> {
    ensure_assigned(&state, trainer.0, session_id).await?;
    let rows: Vec<(Uuid, Uuid, String, Option<String>, String)> = sqlx::query_as(
        r#"SELECT bk.id, u.id, u.full_name, u.phone, bk.status
           FROM booking bk JOIN app_user u ON u.id = bk.student_id
           WHERE bk.session_id = $1 AND bk.status IN ('booked', 'attended', 'no_show')
           ORDER BY u.full_name"#,
    )
    .bind(session_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| StudentInClass {
                booking_id: r.0,
                student_id: r.1,
                full_name: r.2,
                phone: r.3,
                status: r.4,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct AttendanceInput {
    status: String,
}

pub async fn attendance(
    State(state): State<AppState>,
    trainer: AuthTrainer,
    Path(booking_id): Path<Uuid>,
    Json(input): Json<AttendanceInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if input.status != "attended" && input.status != "no_show" {
        return Err(AppError::InvalidInput("invalid_attendance_status"));
    }
    let session: Option<(Uuid,)> = sqlx::query_as("SELECT session_id FROM booking WHERE id = $1")
        .bind(booking_id)
        .fetch_optional(&state.pool)
        .await?;
    ensure_assigned(
        &state,
        trainer.0,
        session.ok_or(AppError::BookingNotFound)?.0,
    )
    .await?;
    let result = sqlx::query(
        "UPDATE booking SET status = $2, attended_at = now(), attendance_marked_by = $3 WHERE id = $1 AND status = 'booked'",
    ).bind(booking_id).bind(&input.status).bind(trainer.0).execute(&state.pool).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::Conflict);
    }
    Ok(Json(serde_json::json!({"updated": true})))
}

#[derive(Deserialize)]
pub struct StudentSearchQuery {
    q: String,
}

#[derive(Serialize)]
pub struct StudentSearchResult {
    id: Uuid,
    full_name: String,
    phone: Option<String>,
}

pub async fn search_students(
    State(state): State<AppState>,
    _trainer: AuthTrainer,
    Query(query): Query<StudentSearchQuery>,
) -> Result<Json<Vec<StudentSearchResult>>, AppError> {
    let q = query.q.trim();
    if q.len() < 2 {
        return Err(AppError::InvalidInput("query_too_short"));
    }
    let like = format!("%{}%", q);
    let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        r#"SELECT id, full_name, phone FROM app_user
           WHERE role = 'student' AND status = 'active'
             AND (full_name ILIKE $1 OR phone ILIKE $1)
           ORDER BY full_name LIMIT 20"#,
    )
    .bind(&like)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| StudentSearchResult {
                id: r.0,
                full_name: r.1,
                phone: r.2,
            })
            .collect(),
    ))
}

pub async fn book_for_student(
    State(state): State<AppState>,
    trainer: AuthTrainer,
    Path((student_id, session_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    ensure_assigned(&state, trainer.0, session_id).await?;
    let id = booking_service::book_class(
        &state.pool,
        student_id,
        session_id,
        trainer.0,
        BookingChannel::Trainer,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"booking_id": id})),
    ))
}

async fn ensure_assigned(
    state: &AppState,
    actor_id: Uuid,
    session_id: Uuid,
) -> Result<(), AppError> {
    let allowed: (bool,) = sqlx::query_as(
        r#"SELECT EXISTS (SELECT 1 FROM class_session cs JOIN app_user u ON u.id = $1
           WHERE cs.id = $2 AND (cs.trainer_id = $1 OR u.role = 'admin'))"#,
    )
    .bind(actor_id)
    .bind(session_id)
    .fetch_one(&state.pool)
    .await?;
    if !allowed.0 {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
