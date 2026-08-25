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

pub async fn htmx() -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/javascript; charset=utf-8"),
            ),
            // Immutable because the version is pinned above: changing htmx means
            // changing this file, which changes the binary.
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        HTMX,
    )
        .into_response()
}
