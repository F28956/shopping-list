//! Static assets, compiled into the binary.
//!
//! Vendored rather than fetched from a CDN: one fewer origin to trust, no third party
//! that can change what runs in a person's browser, and the app keeps working
//! offline. `include_str!` rather than a served directory so there is no filesystem
//! layout to get wrong at deploy time — the bytes travel inside the executable.

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};

/// htmx 2.0.4, from https://unpkg.com/htmx.org@2.0.4/dist/htmx.min.js
const HTMX: &str = include_str!("../assets/htmx.min.js");
/// This application's own stylesheet and behaviour, served rather than inlined so the
/// Content-Security-Policy can forbid inline script and style outright.
const CSS: &str = include_str!("../assets/app.css");
const JS: &str = include_str!("../assets/app.js");

pub async fn css() -> Response {
    served(CSS, "text/css; charset=utf-8", false)
}

pub async fn app_js() -> Response {
    served(JS, "text/javascript; charset=utf-8", false)
}

pub async fn htmx() -> Response {
    // Immutable because the version is pinned above: changing htmx means changing
    // this file, which changes the binary.
    served(HTMX, "text/javascript; charset=utf-8", true)
}

fn served(body: &'static str, content_type: &'static str, immutable: bool) -> Response {
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        // This application's own assets change whenever it is deployed, and nothing in
        // the URL says which version this is. Revalidating is the honest answer until
        // they are fingerprinted.
        "no-cache"
    };

    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        body,
    )
        .into_response()
}
