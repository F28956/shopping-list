//! What this person buys, and the way back from a typo.
//!
//! Read and forget only. History is written as a side effect of adding items — there
//! is no way to teach it something you have not actually bought, which is the point.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use domain::models::item;
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
) -> Result<Json<Vec<item::Name>>, AppError> {
    Ok(Json(
        items::suggestions(&state.ctx, &user.actor(), domain::service::PAGE_MAX).await?,
    ))
}

/// Forgets one remembered item.
///
/// A typo recorded once would otherwise be suggested forever — decay buries it, but
/// burying is not removing.
async fn forget(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    items::forget(&state.ctx, &user.actor(), item::Name(name)).await?;
    Ok(StatusCode::NO_CONTENT)
}
