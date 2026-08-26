//! Who else is on a list.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use domain::models::invite::Token;
use domain::models::list::{self, List, Role};
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

/// Accepting a share link.
///
/// Its own router because it is not nested under a list: the whole point of a link is
/// that the person following it does not have the list yet, and cannot be authorised
/// against one they cannot see.
///
/// This existed only in the browser, which meant a share link could be sent to
/// someone and then only opened on a laptop -- and it is why the API could not be
/// tested for what a viewer is told about their own role.
pub fn invites_router() -> Router<AppState> {
    Router::new().route("/{token}", post(join))
}

async fn join(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(token): Path<String>,
) -> Result<Json<List>, AppError> {
    Ok(Json(
        lists::join(&state.ctx, &user.actor(), &Token(token)).await?,
    ))
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

/// Somebody who can see this list.
///
/// A person rather than an id: a client that shows "user 4" has told you a list is
/// shared and nothing about who with. Names and addresses go only to other members of
/// the same list, who already know.
#[derive(Debug, serde::Serialize)]
pub struct Person {
    pub user_id: i64,
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: Role,
}

async fn members(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
) -> Result<Json<Vec<Person>>, AppError> {
    let people = lists::people_on(&state.ctx, &user.actor(), list::Id(list_id)).await?;

    Ok(Json(
        people
            .into_iter()
            .map(|p| Person {
                user_id: p.user.id.0,
                name: p.user.name.map(|n| n.0),
                email: p.user.email.map(|e| e.0),
                role: p.role,
            })
            .collect(),
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
