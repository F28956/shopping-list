use axum::{extract::FromRequestParts, http::request::Parts};
use domain::models::{session, user};
use domain::service::{Actor, identity, sessions};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};

use crate::error::AppError;
use crate::state::{AppState, AuthMode, Provider};

/// The claims this API cares about, whichever provider they came from.
#[derive(Debug, serde::Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: Option<String>,
    /// Whether the provider says it has checked the address.
    ///
    /// Apple sends this as a string on some tokens and a boolean on others, which is
    /// why it is not a `bool`. Absent is treated as unverified: linking one identity to
    /// another by address is a way into somebody else's shopping, so a claim nobody
    /// vouched for must not reach that check.
    pub email_verified: Option<serde_json::Value>,
    /// Google sends a name. Apple never does — it hands the name to the *client*, once,
    /// in the credential rather than in the token — so an Apple sign-in arrives
    /// nameless and `Person::shown` falls back to the address.
    pub name: Option<String>,
}

impl Claims {
    /// The address, if the provider says it verified it.
    fn verified_email(&self) -> Option<&str> {
        let vouched = match &self.email_verified {
            Some(serde_json::Value::Bool(yes)) => *yes,
            Some(serde_json::Value::String(yes)) => yes == "true",
            _ => false,
        };
        vouched.then(|| self.email.as_deref()).flatten()
    }
}

impl From<Claims> for (user::Sub, Option<user::Name>, Option<user::Email>) {
    fn from(claims: Claims) -> Self {
        (
            user::Sub(claims.sub.clone()),
            claims.name.clone().map(user::Name),
            claims.verified_email().map(|e| user::Email(e.to_string())),
        )
    }
}

/// A request that carries a verified identity.
///
/// Bearer tokens only. This extractor never looks at cookies, and the session layer
/// is never applied to the routes that use it — on a shared origin the browser
/// attaches session cookies to `/api/*` too, so a cookie must not be able to
/// authenticate anything here.
pub struct CurrentUser(pub Actor);

impl CurrentUser {
    /// The actor to hand to the service layer.
    pub fn actor(self) -> Actor {
        self.0
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Bearer(token) = Bearer::from_request_parts(parts, state).await?;

        // A token this server issued, rather than one it was shown. Recognised by
        // shape and not by trying it: sixty-four lowercase hex characters is what
        // `sessions::issue` mints and is not a shape any JWT has, so there is no
        // ambiguity to resolve and no failed provider lookup on every request.
        if is_session_token(&token) {
            return Ok(CurrentUser(
                sessions::resolve(&state.ctx, &session::Token(token)).await?,
            ));
        }

        let token = token.as_str();
        let (provider, claims) = match &state.auth {
            AuthMode::Providers(providers) => verify(token, providers).await?,
            #[cfg(any(test, feature = "test-support"))]
            AuthMode::TrustTheToken => (
                "google",
                Claims {
                    sub: token.to_string(),
                    email: None,
                    email_verified: None,
                    name: None,
                },
            ),
        };

        let (sub, name, email) = claims.into();
        // The one place a transport may reach an identity: resolving it is what
        // produces an actor, so it cannot take one.
        let actor = identity::from_claims(&state.ctx, provider, sub, name, email).await?;

        Ok(CurrentUser(actor))
    }
}

/// Which provider issued this token, and what it says.
///
/// Offered to each in turn until one accepts it whole — signature, issuer, audience
/// and expiry. Nothing here reads a claim to decide which provider to ask, because
/// until a signature has been checked the claims are whatever the sender wrote. The
/// cost of that is one failed verification when a Google token meets Apple first,
/// which is a signature check against a cached key.
///
/// A token no provider accepts is `Unauthorized`, and deliberately says no more: which
/// of the checks failed is the sender's business only in the sense that they should
/// stop.
pub async fn verify(token: &str, providers: &[Provider]) -> Result<(&'static str, Claims), AppError> {
    let kid = decode_header(token)?.kid.ok_or(AppError::Unauthorized)?;

    for provider in providers {
        let Ok(jwk) = provider.jwks.key(&kid).await else {
            continue;
        };

        let mut v = Validation::new(Algorithm::RS256);
        v.set_audience(&provider.audiences);
        v.set_issuer(&provider.issuers);

        let Ok(key) = DecodingKey::from_jwk(&jwk) else {
            continue;
        };

        if let Ok(data) = decode::<Claims>(token, &key, &v) {
            return Ok((provider.name, data.claims));
        }
    }

    Err(AppError::Unauthorized)
}

/// The bearer token, whatever kind it turns out to be.
///
/// Split out from [`CurrentUser`] because `POST /api/sessions` needs the raw token
/// rather than the identity behind it: it is the route that *makes* an identity into a
/// session, so it cannot ask for one first.
///
/// Bearer only. This never looks at cookies, for the reason [`CurrentUser`] gives.
pub struct Bearer(pub String);

impl FromRequestParts<AppState> for Bearer {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|token| Bearer(token.to_string()))
            .ok_or(AppError::Unauthorized)
    }
}

/// Whether this is one of ours.
///
/// Deliberately a shape test rather than a database lookup. Trying the sessions table
/// first and falling back to the providers would put a query in front of every Google
/// request; trying the providers first would put a signature check in front of every
/// Apple one. The two token formats do not overlap, so neither is necessary.
fn is_session_token(token: &str) -> bool {
    token.len() == 64
        && token
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}
