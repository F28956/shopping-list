//! What gets bought on a list, and the way back from a typo.
//!
//! Nested under the list because that is what the memory belongs to: everyone sharing
//! a list shares its history. Read and forget only — history is written as a side
//! effect of adding items, so there is no way to teach it something nobody bought.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use domain::models::{item, list};
use domain::service::items;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        // The whole memory, for a device that resolves lines for itself.
        .route("/entries", get(entries))
        .route("/{name}", axum::routing::delete(forget))
}

/// What has been typed so far, if anything.
#[derive(Debug, serde::Deserialize)]
pub struct Typed {
    pub q: Option<String>,
}

/// The suggestions this person would be offered, in the order they would appear.
///
/// `?q=` narrows them, matched loosely by the service — so the phone and the browser
/// offer the same things for the same letters. Without it, the whole list.
async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Query(typed): Query<Typed>,
) -> Result<Json<Vec<item::Name>>, AppError> {
    Ok(Json(
        items::suggestions(
            &state.ctx,
            &user.actor(),
            list::Id(list_id),
            domain::service::PAGE_MAX,
            typed.q.as_deref(),
        )
        .await?,
    ))
}

/// Everything this list remembers, for a device to keep a copy of.
///
/// Names alone are enough to autocomplete, which is what `/` answers. A client that
/// resolves a typed line for itself needs more: the unit, how much, and where it is
/// filed -- otherwise it reaches a different answer from the server for the same
/// words. See `items::remembered`.
async fn entries(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
) -> Result<Json<Vec<items::Remembered>>, AppError> {
    Ok(Json(
        items::remembered(
            &state.ctx,
            &user.actor(),
            list::Id(list_id),
            domain::service::PAGE_MAX,
        )
        .await?,
    ))
}

/// Forgets one remembered item.
///
/// A typo recorded once would otherwise be suggested forever — decay buries it, but
/// burying is not removing.
async fn forget(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((list_id, name)): Path<(i64, String)>,
) -> Result<StatusCode, AppError> {
    items::forget(
        &state.ctx,
        &user.actor(),
        list::Id(list_id),
        item::Name(name),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
