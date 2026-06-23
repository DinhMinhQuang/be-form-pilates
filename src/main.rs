mod admin;
mod auth;
mod booking;
mod catalog;
mod domain;
mod error;
mod integration;
mod state;
mod student;
mod trainer;

// use tower_http::cors::CorsLayer;

use crate::state::AppState;
use sqlx::postgres::PgPoolOptions;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .expect("Failed to connect to the database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let app = router(AppState { pool });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("listening on :8080");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .merge(admin::routes())
        .merge(auth::routes())
        .merge(catalog::routes())
        // mỗi feature tự khai báo route của nó rồi nest vào đây
        .merge(booking::routes())
        .merge(student::routes())
        .merge(integration::routes())
        .merge(trainer::routes())
        .with_state(state)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received Ctrl+C, starting graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("received SIGTERM, starting graceful shutdown");
        },
    }
}
