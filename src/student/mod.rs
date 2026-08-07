mod handlers;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(handlers::me))
        .route("/sessions", get(handlers::sessions))
        .route("/me/bookings", get(handlers::bookings))
        .route("/me/credits", get(handlers::credits))
        .route("/me/sessions/:id/book", post(handlers::book_session))
        .route("/me/bookings/:id/cancel", post(handlers::cancel_booking))
}
