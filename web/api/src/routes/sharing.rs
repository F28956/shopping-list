//! Who else is on a list.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use domain::models::list::{self, ListMember, Role};
use domain::models::user;
use domain::service::lists;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(members))
        .route("/invites", post(invite).delete(revoke))
        .route("/{user_id}", axum::routing::delete(remove))
}

/// Deserialised straight into the model's `Role`, so `owner` is rejected by the
/// service rather than by a string comparison here.
#[derive(Debug, serde::Deserialize)]
pub struct NewInvite {
    pub role: Role,
}

/// The token, returned exactly once.
///
/// Only its hash is stored, so this response is the only chance to see it — the same
/// trade a password reset makes, and for the same reason.
#[derive(Debug, serde::Serialize)]
pub struct Invitation {
    pub token: String,
    pub role: Role,
}

async fn members(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
) -> Result<Json<Vec<ListMember>>, AppError> {
    Ok(Json(
        lists::members(&state.ctx, &user.actor(), list::Id(list_id)).await?,
    ))
}

async fn invite(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Json(input): Json<NewInvite>,
) -> Result<(StatusCode, Json<Invitation>), AppError> {
    let token = lists::invite(&state.ctx, &user.actor(), list::Id(list_id), input.role).await?;

    Ok((
        StatusCode::CREATED,
        Json(Invitation {
            token: token.0,
            role: input.role,
        }),
    ))
}

/// Withdraws every outstanding invitation to the list.
///
/// All of them, because an owner cannot tell one unused link from another: they saw
/// each token once and never again.
async fn revoke(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
) -> Result<StatusCode, AppError> {
    lists::revoke_invites(&state.ctx, &user.actor(), list::Id(list_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes somebody, or oneself.
async fn remove(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((list_id, user_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    lists::remove_member(
        &state.ctx,
        &user.actor(),
        list::Id(list_id),
        user::Id(user_id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
