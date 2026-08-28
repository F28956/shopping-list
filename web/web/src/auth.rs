//! Who the browser is.
//!
//! The session holds a user id, not a token. It used to hold the raw Google
//! `id_token` and replay it as a bearer token against the API — which meant the
//! session was only good for the hour that token lived, and the handler's answer to
//! expiry was to sign the person out mid-task. Now the identity is resolved once, at
//! the callback, and the session outlives any provider token.

use domain::service::{Actor, Ctx, identity};
use tower_sessions::Session;

use crate::error::AppError;

/// The session key holding the signed-in user's id.
pub const USER_ID: &str = "user_id";

/// The session key holding the list this person was last looking at.
///
/// In the session rather than in the database on purpose: "where I left off" is a
/// property of the device you left off on. A phone and a laptop are allowed to be in
/// different places.
pub const LAST_LIST: &str = "last_list";

/// A share-link token held over a sign-in.
///
/// Somebody who follows a link on a device they have never signed in on has to go to
/// Google and come back, and the token cannot make that trip: it arrives in a URL
/// fragment, which is gone the moment the browser leaves the page. So it waits here,
/// in the session, and the OIDC callback redeems it. Session storage is server-side
/// and the entry is removed as it is used.
pub const PENDING_INVITE: &str = "pending_invite";

/// Loads the signed-in person, if there is one.
///
/// Returns `None` rather than an error when nobody is signed in: the index page is
/// meaningful either way, and only the routes that act need to insist.
pub async fn current_actor(session: &Session, ctx: &Ctx) -> Result<Option<Actor>, AppError> {
    let Some(id): Option<i64> = session.get(USER_ID).await? else {
        return Ok(None);
    };

    match identity::from_session(ctx, id).await? {
        Some(actor) => Ok(Some(actor)),
        // The session outlived the user — a closed account, or a database restored
        // from before they signed up. Treat it as signed out rather than as an error.
        None => {
            session.flush().await?;
            Ok(None)
        }
    }
}

/// The signed-in person, or [`AppError::Unauthenticated`] for routes that need one.
pub async fn require_actor(session: &Session, ctx: &Ctx) -> Result<Actor, AppError> {
    current_actor(session, ctx)
        .await?
        .ok_or(AppError::Unauthenticated)
}
