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
        .route(
            "/trainer/bookings/:id/attendance",
            post(handlers::attendance),
        )
}
