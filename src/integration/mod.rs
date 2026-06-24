mod haravan;

use crate::state::AppState;
use axum::{
    Router,
    routing::{get, post},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/webhooks/haravan/orders/paid", post(haravan::order_paid))
        .route(
            "/webhooks/haravan/orders/paid",
            get(haravan::verify_webhook),
        )
}
