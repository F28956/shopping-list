//! Signing in, for clients whose provider will not keep them signed in.
//!
//! Every other route takes a provider's ID token as the bearer and re-verifies it on
//! each request. That works for Google, whose SDK quietly refreshes the token in the
//! background, and not for Apple, whose identity token lasts about ten minutes and has
//! no silent refresh. The Apple clients therefore trade theirs, once, for a token this
//! server issued.
//!
//! Deliberately not a general-purpose token endpoint: it takes exactly what the
//! bearer path already accepts and gives back something the bearer path also accepts,
//! so there is one notion of "who is asking" and not two.

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use domain::models::session::Token;
use domain::service::{identity, sessions};

use crate::auth::{Bearer, verify};
use crate::error::AppError;
use crate::state::{AppState, AuthMode};

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(open).delete(close))
}

#[derive(serde::Serialize)]
pub struct Issued {
    /// The bearer token from here on. Returned once — only its hash is kept, so a
    /// client that loses it signs in again.
    token: String,
    /// Seconds of idleness after which it stops working, so a client can decide when
    /// to ask again rather than discovering it at the worst moment.
    idle_seconds: i64,
}

/// Exchanges a provider's identity token for one of this server's.
///
/// The provider's token is verified exactly as it would be on any other route, so
/// this endpoint is not a way in that the rest of the API is not.
async fn open(
    State(state): State<AppState>,
    Bearer(token): Bearer,
) -> Result<Json<Issued>, AppError> {
    let (provider, claims) = match &state.auth {
        AuthMode::Providers(providers) => verify(&token, providers).await?,
        #[cfg(any(test, feature = "test-support"))]
        AuthMode::TrustTheToken => (
            "google",
            crate::auth::Claims {
                sub: token.clone(),
                email: None,
                email_verified: None,
                name: None,
            },
        ),
    };

    let (sub, name, email) = claims.into();
    let actor = identity::from_claims(&state.ctx, provider, sub, name, email).await?;

    let issued = sessions::issue(&state.ctx, &actor, provider).await?;

    Ok(Json(Issued {
        token: issued.0,
        idle_seconds: domain::models::session::IDLE_DAYS * 86_400,
    }))
}

/// Signing out on this device, and only on this device.
///
/// Takes the session token as the bearer and ends that one session. Silent about
/// whether there was anything to end — a client that has lost track of its own token
/// should still be able to say "forget this", and answering would make this a way to
/// ask whether a token is real.
async fn close(
    State(state): State<AppState>,
    Bearer(token): Bearer,
) -> Result<StatusCode, AppError> {
    sessions::end(&state.ctx, &Token(token)).await?;
    Ok(StatusCode::NO_CONTENT)
}
