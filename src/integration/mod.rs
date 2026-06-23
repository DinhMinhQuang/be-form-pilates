mod haravan;

use crate::state::AppState;
use axum::{Router, routing::post};

pub fn routes() -> Router<AppState> {
    Router::new().route("/webhooks/haravan/orders/paid", post(haravan::order_paid))
}
