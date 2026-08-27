//! Trading a provider's word for this server's own.
//!
//! Every other route authenticates by re-verifying a provider's token on each request,
//! which works while the provider keeps the client supplied with a fresh one. Google's
//! SDK does. Apple's does not: its identity token lasts about ten minutes and there is
//! no silent refresh, only another prompt — so an Apple client that used the provider's
//! token as its bearer would ask the person to sign in every ten minutes.
//!
//! So the Apple clients sign in once, and swap that token for one of ours. What comes
//! back is opaque, long-lived and revocable, and after that the provider is out of the
//! loop entirely.
//!
//! Nothing here verifies a provider token. The transport does that before it calls in,
//! exactly as for [`super::identity`] — this module only knows what to do once somebody
//! has been established.

use crate::models::session::{Session, Token};

use super::{Actor, Ctx, Result, ServiceError};

/// Issues a session for an actor a transport has just verified.
///
/// The token is returned once and never again: only its hash is stored, so a client
/// that loses it signs in again rather than asking for it back.
pub async fn issue(ctx: &Ctx, actor: &Actor, provider: &str) -> Result<Token> {
    let person = actor.person()?;

    let token = Token(new_token());
    Session::create(&ctx.db, &token, person.id, provider).await?;

    tracing::info!(user = person.id.0, %provider, "session issued");
    Ok(token)
}

/// 256 bits from the operating system, hex-encoded.
///
/// The same reasoning as an invite's token, and more so: this one is the whole of the
/// credential for months rather than a week.
fn new_token() -> String {
    use rand::Rng;

    // ThreadRng, which is a cryptographic generator seeded from the operating system —
    // not one of the fast reproducible ones. A predictable token is no token.
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Who is holding this session token.
///
/// Admission is checked here and not only at sign-in, for the reason `from_session`
/// gives: a session that outlived somebody's removal from the list would mean taking an
/// address off it had no effect for three months.
pub async fn resolve(ctx: &Ctx, token: &Token) -> Result<Actor> {
    // Unauthenticated and not `NotFound`: the caller asked who they are, not for a
    // thing. A 404 here would tell a client to give up on a route rather than to sign
    // in again, which is the difference between a session that expired and an API that
    // moved.
    let user_id = Session::claim(&ctx.db, token)
        .await
        .map_err(|_| ServiceError::Unauthenticated)?;

    super::identity::from_session(ctx, user_id.0)
        .await?
        .ok_or(ServiceError::NotAdmitted)
}

/// Ends this one session. The other devices stay signed in — signing out on a phone
/// that is being handed on should not sign out the Mac at home.
pub async fn end(ctx: &Ctx, token: &Token) -> Result<()> {
    Session::revoke(&ctx.db, token).await?;
    Ok(())
}
