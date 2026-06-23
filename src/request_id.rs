use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tracing::Instrument;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Debug, Clone)]
pub struct RequestId(pub String);

pub async fn request_id(mut req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    if let Ok(header_value) = HeaderValue::from_str(&request_id) {
        req.headers_mut()
            .insert(REQUEST_ID_HEADER.clone(), header_value);
    }
    req.extensions_mut().insert(RequestId(request_id.clone()));

    let span = tracing::info_span!("http.request", request_id = %request_id);
    let mut response = next.run(req).instrument(span).await;
    if let Ok(header_value) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER.clone(), header_value);
    }

    response
}
