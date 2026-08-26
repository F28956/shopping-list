//! The HTTP API: bearer-authenticated JSON, for clients that are not this server's
//! own web UI.
//!
//! Exports a router rather than serving one. Which port it lands on, what it is
//! nested under and which layers wrap it are the `server` crate's decisions — this
//! crate only knows how to translate HTTP into service calls.

pub mod auth;
pub mod error;
pub mod jwks;
pub mod routes;
pub mod state;

use axum::Router;

use crate::state::AppState;

/// The API's routes, relative to wherever they are nested.
///
/// The caller must NOT wrap this in a session layer. Every route here
/// authenticates from `Authorization: Bearer` and nothing else, and on a shared
/// origin the browser will attach its session cookie to these paths too — see
/// [`auth::CurrentUser`].
pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/lists", routes::lists::router())
        // Items are nested under their list because the list is what authorises them
        .nest("/lists/{list_id}/items", routes::items::router())
        // The memory belongs to the list, so it is addressed through it
        .nest("/lists/{list_id}/history", routes::history::router())
        .nest("/lists/{list_id}/members", routes::sharing::router())
        // Not under a list: following a link is what gets you the list.
        .nest("/invites", routes::sharing::invites_router())
        .nest("/me", routes::me::router())
        .nest("/notes", routes::notes::router())
        // One route for everything a device did while it was away -- see the module.
        .nest("/sync", routes::sync::router())
        .nest("/units", routes::reference::units_router())
        .nest("/tags", routes::reference::tags_router())
}
