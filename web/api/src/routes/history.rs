//! What gets bought on a list, and the way back from a typo.
//!
//! Nested under the list because that is what the memory belongs to: everyone sharing
//! a list shares its history. Read and forget only — history is written as a side
//! effect of adding items, so there is no way to teach it something nobody bought.

use axum::{
    Json, Router,
    extract::{Path, State},
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
        .route("/{name}", axum::routing::delete(forget))
}

/// The suggestions this person would be offered, in the order they would appear.
async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
) -> Result<Json<Vec<item::Name>>, AppError> {
    Ok(Json(
        items::suggestions(
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
