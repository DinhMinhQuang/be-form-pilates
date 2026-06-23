// Cả feature booking nằm trong một thư mục. Sửa luồng đặt lịch chỉ mở đúng folder này.
mod handlers;
mod queries;
pub(crate) mod service;

use axum::{Router, routing::post};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/:id/book", post(handlers::book))
        .route("/bookings/:id/cancel", post(handlers::cancel))
}
