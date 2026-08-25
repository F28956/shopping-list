//! Turning a verified identity into an [`Actor`].
//!
//! This is the one sanctioned exception to the rule that transports do not touch
//! models: resolving who someone is cannot take an `Actor`, because it is what
//! *produces* one. It lives here so the exception is single and visible rather than
//! repeated in every transport that authenticates.
//!
//! Nothing here verifies anything. A bearer token's signature and a session cookie's
//! integrity are the transports' business; by the time a caller reaches this module
//! it has already established *that* the identity is genuine, and only needs to know
//! *who* it belongs to.

use crate::models::user::{self, Email, Name, Sub, User};

use super::{Actor, Ctx, Result};

/// Resolves the identity behind a verified provider token, creating the user on first
/// sight.
///
/// Idempotent, because it runs on every authenticated request — see
/// [`User::find_or_create`].
pub async fn from_claims(
    ctx: &Ctx,
    sub: Sub,
    name: Option<Name>,
    email: Option<Email>,
) -> Result<Actor> {
    let user = User::find_or_create(&ctx.db, sub, name, email).await?;
    Ok(Actor::User(user))
}

/// Resolves the person a session belongs to.
///
/// `None` where the session outlived the user — a closed account, or a database
/// restored from before they signed up. That is a signed-out visitor, not an error:
/// the caller flushes the session and carries on.
pub async fn from_session(ctx: &Ctx, user_id: i64) -> Result<Option<Actor>> {
    match User::get(&ctx.db, user::Lookup::Id(user::Id(user_id))).await {
        Ok(user) => Ok(Some(Actor::User(user))),
        Err(crate::models::Error::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
