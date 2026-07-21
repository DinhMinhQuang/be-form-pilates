mod admin;
mod auth;
mod booking;
mod catalog;
mod domain;
mod email;
mod error;
mod integration;
mod middleware;
mod request_id;
mod state;
mod student;
mod trainer;

use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;
use axum::extract::Request;
use axum::http::{Method, Response, header};
use axum::middleware as axum_middleware;
use sqlx::postgres::PgPoolOptions;
use tokio::signal;
use tracing::Span;

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

    let sync_pool = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(15 * 60));
        loop {
            interval.tick().await;
            integration::haravan_sync::sync_products(&sync_pool).await;
        }
    });

    let sweep_pool = pool.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5 * 60));
        loop {
            interval.tick().await;
            match booking::service::sweep_completed_sessions(&sweep_pool).await {
                Ok(n) if n > 0 => tracing::info!(count = n, "sessions swept to completed"),
                Ok(_) => {}
                Err(e) => tracing::error!(error = %e, "failed to sweep completed sessions"),
            }
        }
    });

    tokio::spawn(email::start_worker(pool.clone()));

    let app = router(AppState { pool });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3355").await?;
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
        .merge(booking::routes())
        .merge(student::routes())
        .merge(integration::routes())
        .merge(trainer::routes())
        .layer(axum_middleware::from_fn(crate::middleware::log_body))
        .layer(
            TraceLayer::new_for_http()
                .on_request(|req: &Request<_>, _span: &Span| {
                    let request_id = req
                        .extensions()
                        .get::<request_id::RequestId>()
                        .map(|id| id.0.as_str());
                    tracing::info!(
                        request_id = ?request_id,
                        method = %req.method(),
                        path = %req.uri().path(),
                        query = ?req.uri().query(),
                        headers = ?req.headers(),
                        "→ request"
                    );
                })
                .on_response(|res: &Response<_>, latency: Duration, _span: &Span| {
                    tracing::info!(
                        status = %res.status(),
                        latency = ?latency,
                        "← response"
                    );
                })
                .on_failure(|error, latency: Duration, _span: &Span| {
                    tracing::error!(
                        error = %error,
                        latency = ?latency,
                        "❌ error"
                    );
                }),
        )
        .layer(axum_middleware::from_fn(request_id::request_id))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                    Method::HEAD,
                ])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
        )
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
