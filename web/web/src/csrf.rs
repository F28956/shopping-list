//! Refusing state-changing requests that another site set off.
//!
//! Every mutation here is a cookie-authenticated POST, which is the shape CSRF
//! attacks. `SameSite=Lax` already withholds the cookie from cross-site POSTs, and
//! that is a real defence — but it was the *only* one, it depends on the browser, and
//! it quietly stops applying the day someone sets `SameSite=None` for an embed or
//! adds a permissive CORS layer.
//!
//! This is the second layer, and it needs no token threaded through every form:
//! browsers state where a request came from, and a request that says it came from
//! somewhere else is refused.
//!
//! A request that says nothing is allowed. `curl` sends no `Origin`, and neither does
//! any non-browser client — but neither can be tricked into attaching somebody's
//! cookie either, which is what makes CSRF possible in the first place.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The origin this application is served from, for comparing against `Origin`.
///
/// Read once at startup rather than per request: it is deployment configuration, and
/// a request is not the place to discover it.
#[derive(Debug, Clone)]
pub struct Origin(pub String);

impl Origin {
    /// From `PUBLIC_ORIGIN`, falling back to the address a local run serves on.
    pub fn from_env() -> Self {
        Origin(
            std::env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        )
    }
}

/// Rejects unsafe methods that a different site initiated.
pub async fn guard(State(origin): State<Origin>, request: Request<Body>, next: Next) -> Response {
    if is_safe(request.method()) || came_from_here(&origin, request.headers()) {
        return next.run(request).await;
    }

    tracing::warn!(
        method = %request.method(),
        uri = %request.uri(),
        "refused a cross-site state-changing request"
    );
    (StatusCode::FORBIDDEN, "cross-site request refused").into_response()
}

/// GET, HEAD and OPTIONS change nothing, so they are not worth refusing — and
/// refusing them would break ordinary links from other sites.
fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn came_from_here(origin: &Origin, headers: &HeaderMap) -> bool {
    // Modern browsers say this outright, and say it for navigations too.
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return matches!(site, "same-origin" | "none");
    }

    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(sent) => sent == origin.0,
        // No browser said anything, so no browser is involved.
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    fn here() -> Origin {
        Origin("https://list.example.com".to_string())
    }

    #[test]
    fn a_matching_origin_passes() {
        assert!(came_from_here(
            &here(),
            &headers(&[("origin", "https://list.example.com")])
        ));
    }

    #[test]
    fn another_site_is_refused() {
        assert!(!came_from_here(
            &here(),
            &headers(&[("origin", "https://evil.example")])
        ));
    }

    /// The header browsers send even when they omit `Origin`.
    #[test]
    fn sec_fetch_site_is_believed_first() {
        assert!(came_from_here(
            &here(),
            &headers(&[
                ("sec-fetch-site", "same-origin"),
                ("origin", "https://evil.example")
            ])
        ));
        assert!(!came_from_here(
            &here(),
            &headers(&[
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://list.example.com")
            ])
        ));
    }

    /// A typed URL or a bookmark: no site initiated it.
    #[test]
    fn a_direct_navigation_passes() {
        assert!(came_from_here(
            &here(),
            &headers(&[("sec-fetch-site", "none")])
        ));
    }

    /// Nothing said anything, so nothing can be carrying a stolen cookie.
    #[test]
    fn a_non_browser_client_passes() {
        assert!(came_from_here(&here(), &headers(&[])));
    }

    #[test]
    fn reads_are_never_refused() {
        assert!(is_safe(&Method::GET));
        assert!(is_safe(&Method::HEAD));
        assert!(!is_safe(&Method::POST));
        assert!(!is_safe(&Method::DELETE));
    }
}
