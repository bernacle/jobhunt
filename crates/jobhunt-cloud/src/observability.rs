//! Request ids and request tracing.
//!
//! Every request gets an id (`x-request-id`, taken from the proxy when it
//! sends one) that is returned in the response and attached to every log
//! line of the request, with the method, the route *template* (not the raw
//! path or query string) and, once authenticated, the internal account id.
//! Bodies, headers (`authorization`, cookies) and query strings are never
//! logged.

use std::time::Duration;

use axum::extract::MatchedPath;
use http::{HeaderName, Request, Response};
use tower_http::request_id::{MakeRequestId, RequestId};
use tracing::Span;

pub const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Random request ids (`req_<16 hex>`).
#[derive(Debug, Clone, Copy, Default)]
pub struct RandomRequestId;

impl MakeRequestId for RandomRequestId {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let mut bytes = [0u8; 8];
        getrandom_bytes(&mut bytes);
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        http::HeaderValue::from_str(&format!("req_{id}"))
            .ok()
            .map(RequestId::new)
    }
}

fn getrandom_bytes(buf: &mut [u8]) {
    // Uniqueness, not secrecy: time and a counter are enough and need no
    // extra dependency.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    let mixed = (t ^ n.rotate_left(32) ^ u64::from(std::process::id())).to_be_bytes();
    for (slot, byte) in buf.iter_mut().zip(mixed) {
        *slot = byte;
    }
}

/// The span of one request (safe fields only).
pub fn make_span<B>(request: &Request<B>) -> Span {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("(unmatched)", MatchedPath::as_str)
        .to_owned();
    let id = request
        .headers()
        .get(REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    tracing::info_span!(
        "request",
        request_id = %id,
        method = %request.method(),
        route = %route,
        user = tracing::field::Empty,
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Logs the outcome of a request.
pub fn on_response<B>(response: &Response<B>, latency: Duration, span: &Span) {
    span.record("status", response.status().as_u16());
    span.record("duration_ms", latency.as_millis() as u64);
    if response.status().is_server_error() {
        tracing::warn!("request failed");
    } else {
        tracing::info!("request finished");
    }
}
