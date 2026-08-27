//! Who may use this server, and who decides.
//!
//! Every route here is refused to somebody who merely uses the server, and the refusal
//! is decided in `domain::service::admission` rather than here — see D1. This module
//! translates and nothing else.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use domain::models::admission::{Admitted, Note, Server};
use domain::models::user::Email;
use domain::service::{admission, sessions};

use crate::auth::{Bearer, CurrentUser, verify};
use crate::error::AppError;
use crate::state::{AppState, AuthMode};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(listing).post(admit))
        .route("/{email}", axum::routing::delete(withdraw))
        .route("/{email}/owner", post(promote).delete(demote))
}

/// What a client may know before anybody has signed in.
///
/// Unauthenticated on purpose, and it says only what a sign-in screen needs in order
/// to be honest: whether to offer the claim, and whether to promise a refusal that
/// will not come. It carries nothing about who is here and never the claim code.
pub fn server_router() -> Router<AppState> {
    Router::new().route("/", get(about).put(set_open)).route("/claim", post(claim))
}

#[derive(serde::Serialize)]
struct About {
    /// False only in the window between a first boot and somebody claiming it, when
    /// the client should be asking for the code from the log instead of signing in.
    claimed: bool,
    admits_anyone: bool,
}

async fn about(State(state): State<AppState>) -> Result<Json<About>, AppError> {
    Ok(Json(About {
        claimed: Server::is_claimed(&state.ctx.db).await?,
        admits_anyone: Server::admits_anyone(&state.ctx.db).await?,
    }))
}

async fn listing(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<Admitted>>, AppError> {
    Ok(Json(admission::listing(&state.ctx, &user.actor()).await?))
}

#[derive(serde::Deserialize)]
struct Admitting {
    email: String,
    /// "mum", so that a list of addresses stays readable.
    note: Option<String>,
}

async fn admit(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<Admitting>,
) -> Result<StatusCode, AppError> {
    admission::admit(
        &state.ctx,
        &user.actor(),
        &Email(body.email),
        body.note.map(Note).as_ref(),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn withdraw(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(email): Path<String>,
) -> Result<StatusCode, AppError> {
    admission::withdraw(&state.ctx, &user.actor(), &Email(email)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn promote(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(email): Path<String>,
) -> Result<StatusCode, AppError> {
    admission::set_ownership_of(&state.ctx, &user.actor(), &Email(email), true).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn demote(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(email): Path<String>,
) -> Result<StatusCode, AppError> {
    admission::set_ownership_of(&state.ctx, &user.actor(), &Email(email), false).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct Opening {
    admits_anyone: bool,
}

async fn set_open(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<Opening>,
) -> Result<StatusCode, AppError> {
    admission::set_open(&state.ctx, &user.actor(), body.admits_anyone).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct Claiming {
    code: String,
}

#[derive(serde::Serialize)]
struct Claimed {
    token: String,
}

/// Claims a server nobody owns.
///
/// Takes a provider token as the bearer, exactly as `POST /api/sessions` does, and
/// hands back a session the same way — so the person who claims the server is signed
/// in by the act of claiming it rather than by a second round trip that could fail
/// while they are the owner of a server they cannot reach.
///
/// It cannot use [`CurrentUser`], and that is the whole point: resolving an identity
/// checks admission, and on an unclaimed server nobody is admitted because nobody has
/// admitted them.
async fn claim(
    State(state): State<AppState>,
    Bearer(token): Bearer,
    Json(body): Json<Claiming>,
) -> Result<Json<Claimed>, AppError> {
    let (provider, claims) = match &state.auth {
        AuthMode::Providers(providers) => verify(&token, providers).await?,
        #[cfg(any(test, feature = "test-support"))]
        AuthMode::TrustTheToken => (
            "google",
            // An address, unlike the other test branch, because claiming admits the
            // owner by theirs and an owner with no address is an owner who cannot
            // sign in.
            crate::auth::Claims {
                sub: token.clone(),
                email: Some(format!("{token}@example.com")),
                email_verified: Some(serde_json::Value::Bool(true)),
                name: None,
            },
        ),
    };

    let (sub, name, email) = claims.into();
    let actor = admission::claim(&state.ctx, &body.code, provider, sub, name, email).await?;
    let issued = sessions::issue(&state.ctx, &actor, provider).await?;

    Ok(Json(Claimed { token: issued.0 }))
}
