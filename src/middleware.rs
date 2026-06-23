use axum::{body::Body, extract::Request, middleware::Next, response::Response};

const MAX_LOG_BYTES: usize = 4096;

pub async fn log_body(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let request_id = req
        .extensions()
        .get::<crate::request_id::RequestId>()
        .map(|id| id.0.clone())
        .unwrap_or_default();

    let (req_parts, req_body) = req.into_parts();
    let req_bytes = axum::body::to_bytes(req_body, usize::MAX)
        .await
        .unwrap_or_default();
    tracing::info!(request_id = %request_id, body = %fmt(&req_bytes), "→ body");

    let req = Request::from_parts(req_parts, Body::from(req_bytes));
    let response = next.run(req).await;

    let (res_parts, res_body) = response.into_parts();
    let res_bytes = axum::body::to_bytes(res_body, usize::MAX)
        .await
        .unwrap_or_default();
    tracing::info!(
        request_id = %request_id,
        method = %method,
        uri = %uri,
        status = %res_parts.status,
        body = %fmt(&res_bytes),
        "← body"
    );

    Response::from_parts(res_parts, Body::from(res_bytes))
}

fn fmt(bytes: &axum::body::Bytes) -> String {
    if bytes.is_empty() {
        return "(empty)".to_string();
    }
    if bytes.len() > MAX_LOG_BYTES {
        format!(
            "{}... ({} bytes)",
            String::from_utf8_lossy(&bytes[..MAX_LOG_BYTES]),
            bytes.len()
        )
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}
