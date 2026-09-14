//! Bearer-token middleware for `/v1/*`.

use axum::{
    extract::State,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use crate::routes::AppState;

pub async fn require_bearer(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let header_value = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let supplied = match header_value {
        Some(s) if s.starts_with("Bearer ") => &s[7..],
        _ => return unauthorized(),
    };

    if constant_time_eq(supplied.as_bytes(), state.token.as_bytes()) {
        next.run(req).await
    } else {
        unauthorized()
    }
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
}

/// Constant-time comparison so the token can't be probed via timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
