mod handlers;

use crate::state::AppState;
use axum::{
    Router,
    routing::{get, post},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/trainer/sessions", get(handlers::sessions))
        .route("/trainer/sessions/:id/students", get(handlers::students))
        .route("/trainer/students", get(handlers::search_students))
        .route("/trainer/bookings/:id/attendance", post(handlers::attendance))
        .route(
            "/trainer/students/:student_id/sessions/:session_id/book",
            post(handlers::book_for_student),
        )
}
