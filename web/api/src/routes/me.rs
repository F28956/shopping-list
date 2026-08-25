//! Who you are.
//!
//! Read-only. `users::update_profile` exists and is tested, but wiring it up would
//! ship a setting that silently does not stick: authentication resolves the identity
//! through `User::find_or_create` on *every* request, and that coalesces the
//! provider's claims over what is stored. Google sends a name and an address every
//! time, so a profile a person edited here would be overwritten by their next
//! request. Deciding which side wins is a real decision, and until it is made this
//! stays read-only rather than pretending.
//!
//! `users::close_account` is likewise not wired: it cascades away every list, item
//! and note the person owns, and an irreversible DELETE deserves a confirmation flow
//! designed on purpose rather than a route added in passing.

use axum::{Json, Router, extract::State, routing::get};
use domain::models::user::User;
use domain::service::users;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(me))
}

/// The signed-in person.
///
/// There is no `/api/users/{id}`: nothing here needs one person to look another up,
/// and an endpoint that could would be the first thing to leak an address.
async fn me(State(state): State<AppState>, user: CurrentUser) -> Result<Json<User>, AppError> {
    // Through the service, not straight off the extractor: this is the only route
    // that could plausibly skip it, and the moment one does the rule stops being one.
    Ok(Json(users::me(&state.ctx, &user.actor()).await?))
}
