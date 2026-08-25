use axum::{extract::FromRequestParts, http::request::Parts};
use domain::models::user::{self, User};
use domain::service::Actor;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};

use crate::error::AppError;
use crate::state::{AppState, AuthMode};

/// The claims this API cares about, whichever provider they came from.
#[derive(Debug, serde::Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

impl From<Claims> for (user::Sub, Option<user::Name>, Option<user::Email>) {
    fn from(claims: Claims) -> Self {
        (
            user::Sub(claims.sub),
            claims.name.map(user::Name),
            claims.email.map(user::Email),
        )
    }
}

/// A request that carries a verified identity.
///
/// Bearer tokens only. This extractor never looks at cookies, and the session layer
/// is never applied to the routes that use it — on a shared origin the browser
/// attaches session cookies to `/api/*` too, so a cookie must not be able to
/// authenticate anything here.
pub struct CurrentUser(pub User);

impl CurrentUser {
    /// The actor to hand to the service layer.
    pub fn actor(self) -> Actor {
        Actor::User(self.0)
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;

        let claims = match &state.auth {
            AuthMode::Google { jwks, client_id } => {
                let kid = decode_header(token)?.kid.ok_or(AppError::Unauthorized)?;
                let jwk = jwks.key(&kid).await?;

                let mut v = Validation::new(Algorithm::RS256);
                v.set_audience(&[client_id]);
                v.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

                decode::<Claims>(token, &DecodingKey::from_jwk(&jwk)?, &v)?.claims
            }
            #[cfg(test)]
            AuthMode::TrustTheToken => Claims {
                sub: token.to_string(),
                email: None,
                name: None,
            },
        };

        let (sub, name, email) = claims.into();
        // find_or_create, not create: this runs on every authenticated request, so it
        // has to be idempotent. `create` would collide with `users.sub UNIQUE` the
        // second time a returning person made a request.
        let user = User::find_or_create(&state.ctx.db, sub, name, email).await?;

        Ok(CurrentUser(user))
    }
}
