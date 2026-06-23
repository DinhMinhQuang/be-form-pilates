mod handlers;

use crate::state::AppState;
use axum::{
    Router,
    routing::{get, patch, post, put},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/sessions",
            get(handlers::sessions).post(handlers::create_session),
        )
        .route(
            "/admin/sessions/:id",
            patch(handlers::update_session).delete(handlers::cancel_session),
        )
        .route(
            "/admin/trainers",
            get(handlers::trainers).post(handlers::create_trainer),
        )
        .route(
            "/admin/trainers/:id",
            patch(handlers::update_trainer).delete(handlers::delete_trainer),
        )
        .route(
            "/admin/students",
            get(handlers::students).post(handlers::create_student),
        )
        .route(
            "/admin/students/:id",
            get(handlers::student_detail).patch(handlers::update_student),
        )
        .route("/admin/bookings", get(handlers::bookings))
        .route(
            "/admin/students/:student_id/sessions/:session_id/book",
            post(handlers::book_for_student),
        )
        .route(
            "/admin/bookings/:id/cancel",
            post(handlers::cancel_for_student),
        )
        .route(
            "/admin/students/:student_id/credits/:lot_id",
            patch(handlers::adjust_credit),
        )
        .route(
            "/admin/haravan/product-mappings",
            get(handlers::product_mappings).post(handlers::create_product_mapping),
        )
        .route(
            "/admin/haravan/product-mappings/:id",
            patch(handlers::update_product_mapping),
        )
        .route(
            "/admin/packages/:id/class-types",
            put(handlers::set_package_class_types),
        )
}
