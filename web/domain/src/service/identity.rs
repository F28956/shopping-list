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

use super::admission;
use super::{Actor, Ctx, Result, ServiceError};

/// Resolves the identity behind a verified provider token, creating the user on first
/// sight.
///
/// Idempotent, because it runs on every authenticated request.
///
/// `provider` is which service vouched for the subject. It is part of the key: a
/// subject is only unique within the provider that issued it, and the same person
/// arrives with a different one depending on which device is in their hand — Apple on
/// the phone and the Mac, Google on Android.
pub async fn from_claims(
    ctx: &Ctx,
    provider: &str,
    sub: Sub,
    name: Option<Name>,
    email: Option<Email>,
) -> Result<Actor> {
    // Somebody this server has seen before, on this provider. Answered first, and
    // without consulting the token's email, because Apple stops sending one after the
    // first authorisation -- admission that read the token alone would let a person in
    // once and refuse them for ever after.
    if let Some(known) = User::by_identity(&ctx.db, provider, &sub).await? {
        if !admission::admits_user(ctx, known.id).await? {
            tracing::warn!(user = known.id.0, "sign-in refused: no longer an admitted address");
            return Err(ServiceError::NotAdmitted);
        }

        // Still coalesced, so a name or an address the provider has started sending is
        // picked up. `find_or_create` keys on `users.sub`, which for an attached
        // identity is not this subject -- so the update goes through the id.
        let refreshed = match (name, email) {
            (None, None) => known,
            (name, email) => User::refresh(&ctx.db, known.id, name, email).await?,
        };
        return Ok(Actor::User(refreshed));
    }

    // A new identity. From here the token's email is all there is, and Apple does send
    // it on a first authorisation -- which is the only time this branch runs.
    if !admission::admits_email(ctx, email.as_ref()).await? {
        tracing::warn!(%provider, sub = %sub.0, "sign-in refused: not an admitted address");
        return Err(ServiceError::NotAdmitted);
    }

    // The same person, arriving the other way. Somebody who signed in with Google on
    // their phone and with Apple on their laptop is one person with one list, and
    // matching on the address is what says so.
    //
    // Only an address the provider vouches for reaches this far -- see the transports.
    // Apple's "Hide my email" gives a relay address instead, which matches nothing and
    // is refused by admission above anyway, so a hidden sign-in is a new account or no
    // account rather than a way into somebody else's.
    if let Some(email) = email.clone()
        && let Some(existing) = User::by_email(&ctx.db, &email).await?
    {
        User::attach_identity(&ctx.db, provider, &sub, existing.id).await?;
        tracing::info!(user = existing.id.0, %provider, "identity attached by address");
        let refreshed = User::refresh(&ctx.db, existing.id, name, Some(email.clone())).await?;
        admission::bind(ctx, Some(&email), existing.id).await?;
        return Ok(Actor::User(refreshed));
    }

    // Qualified, because `users.sub` is unique across the whole table and a subject is
    // only unique within the provider that issued it. Unqualified, an Apple subject
    // that happened to match a Google one would land on `ON CONFLICT(sub)` and hand
    // somebody else's account to a stranger.
    //
    // Only new accounts are qualified. The ones that predate two providers keep the
    // raw subject they were created with, and are found by their identity row rather
    // than by this column -- which is why nothing had to be rewritten.
    let qualified = user::Sub(format!("{provider}|{}", sub.0));
    let user = User::find_or_create(&ctx.db, qualified, name, email.clone()).await?;
    User::attach_identity(&ctx.db, provider, &sub, user.id).await?;
    // Binds the address to the person, so that from here on admission follows them
    // rather than the address -- see `models::admission`.
    admission::bind(ctx, email.as_ref(), user.id).await?;
    Ok(Actor::User(user))
}

/// Resolves the person a session belongs to.
///
/// `None` where the session outlived the user — a closed account, or a database
/// restored from before they signed up. That is a signed-out visitor, not an error:
/// the caller flushes the session and carries on.
///
/// Someone taken off the admission list is `None` too, and for the same reason:
/// checking only at sign-in would mean removing an address had no effect until their
/// cookie happened to expire. Signed-out rather than forbidden so it heals itself —
/// they land on the sign-in page, and signing in again is what tells them no.
pub async fn from_session(ctx: &Ctx, user_id: i64) -> Result<Option<Actor>> {
    match User::get(&ctx.db, user::Lookup::Id(user::Id(user_id))).await {
        Ok(user) if !admission::admits_user(ctx, user.id).await.unwrap_or(false) => {
            tracing::warn!(user = user.id.0, "session dropped: no longer admitted");
            Ok(None)
        }
        Ok(user) => Ok(Some(Actor::User(user))),
        Err(crate::models::Error::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
