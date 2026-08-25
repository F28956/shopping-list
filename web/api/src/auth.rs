use axum::{extract::FromRequestParts, http::request::Parts};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};

use crate::error::AppError;
use crate::models::user;
use crate::models::user::User;
use crate::state::AppState;

#[derive(serde::Deserialize)]
struct GoogleClaims {
    sub: String,
    email: Option<String>,
    name: Option<String>,
}

pub struct CurrentUser(pub User);

impl From<GoogleClaims> for (user::Sub, Option<user::Name>, Option<user::Email>) {
    fn from(claims: GoogleClaims) -> Self {
        (
            user::Sub(claims.sub),
            claims.name.map(user::Name),
            claims.email.map(user::Email),
        )
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

        let kid = decode_header(token)?.kid.ok_or(AppError::Unauthorized)?;
        let jwk = state.jwks.key(&kid).await?;

        let mut v = Validation::new(Algorithm::RS256);
        v.set_audience(&[&state.google_client_id]);
        v.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

        let claims = decode::<GoogleClaims>(token, &DecodingKey::from_jwk(&jwk)?, &v)?.claims;

        let (sub, name, email) = claims.into();
        let user = User::create(&state.db, sub, name, email).await?;

        Ok(CurrentUser(user))
    }
}
