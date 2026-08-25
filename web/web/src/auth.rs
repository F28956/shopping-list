//! Who the browser is.
//!
//! The session holds a user id, not a token. It used to hold the raw Google
//! `id_token` and replay it as a bearer token against the API — which meant the
//! session was only good for the hour that token lived, and the handler's answer to
//! expiry was to sign the person out mid-task. Now the identity is resolved once, at
//! the callback, and the session outlives any provider token.

use domain::models::user::{self, User};
use domain::service::{Actor, Ctx};
use tower_sessions::Session;

use crate::error::AppError;

/// The session key holding the signed-in user's id.
pub const USER_ID: &str = "user_id";

/// Loads the signed-in person, if there is one.
///
/// Returns `None` rather than an error when nobody is signed in: the index page is
/// meaningful either way, and only the routes that act need to insist.
pub async fn current_user(session: &Session, ctx: &Ctx) -> Result<Option<User>, AppError> {
    let Some(id): Option<i64> = session.get(USER_ID).await? else {
        return Ok(None);
    };

    match User::get(&ctx.db, user::Lookup::Id(user::Id(id))).await {
        Ok(user) => Ok(Some(user)),
        // The session outlived the user — deleted account, or a database restored
        // from before they signed up. Treat it as signed out rather than as an error.
        Err(domain::models::Error::NotFound) => {
            session.flush().await?;
            Ok(None)
        }
        Err(e) => Err(AppError::Service(e.into())),
    }
}

/// The signed-in person, or [`AppError::Unauthenticated`] for routes that need one.
pub async fn require_actor(session: &Session, ctx: &Ctx) -> Result<Actor, AppError> {
    current_user(session, ctx)
        .await?
        .map(Actor::User)
        .ok_or(AppError::Unauthenticated)
}
