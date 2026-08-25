//! Responding to htmx without giving up on plain HTML.
//!
//! Every mutating route still works with JavaScript switched off: the forms keep
//! their `method` and `action`, and a request that did not come from htmx gets the
//! redirect it always got. When htmx *is* driving, the same handler returns just the
//! fragment that changed instead, so the page does not reload.
//!
//! Keeping both paths costs one branch per handler and means the no-JS behaviour is
//! not a story we tell — it is the default, and the existing tests exercise it.

use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

/// Whether this request came from htmx rather than from a browser form post.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// A fragment for htmx, or a redirect for everyone else.
pub fn swap_or_redirect(headers: &HeaderMap, fragment: Markup, to: &str) -> Response {
    if is_htmx(headers) {
        fragment.into_response()
    } else {
        Redirect::to(to).into_response()
    }
}
