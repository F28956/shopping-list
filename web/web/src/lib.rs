//! The server-rendered web UI: cookie-authenticated HTML, for browsers.
//!
//! Exports a router rather than serving one. The session layer is applied here, on
//! this crate's routes only — never to the combined router — so that a session cookie
//! can never authenticate an API route it happens to share an origin with.

use axum::{
    Router,
    extract::{Query, State},
    response::Redirect,
    routing::{get, post},
};
use domain::models::user::{self, User};
use domain::service::Ctx;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use std::sync::Arc;
use tower_sessions::{Expiry, Session, SessionManagerLayer, cookie::SameSite};

pub mod auth;
pub mod error;
pub mod pages;
pub mod sessions;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod view;
pub use error::AppError;

pub mod state;
pub use state::{AppState, CallbackQuery};

async fn login(session: Session, State(s): State<AppState>) -> Result<Redirect, AppError> {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf, nonce) = s
        .oidc
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(challenge)
        .url();

    session.insert("pkce", verifier.secret().clone()).await?;
    session.insert("csrf", csrf.secret().clone()).await?;
    session.insert("nonce", nonce.secret().clone()).await?;

    tracing::debug!("redirecting to google");
    Ok(Redirect::to(auth_url.as_str()))
}

async fn logout(session: Session) -> Result<Redirect, AppError> {
    session.flush().await?;
    Ok(Redirect::to("/"))
}

async fn callback(
    session: Session,
    State(s): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Result<Redirect, AppError> {
    // 1. Recover and consumer what has been stashed in /auth/login
    let csrf: String = session.remove("csrf").await?.ok_or(AppError::BadRequest)?;
    let verifier: String = session.remove("pkce").await?.ok_or(AppError::BadRequest)?;
    let nonce: String = session.remove("nonce").await?.ok_or(AppError::BadRequest)?;

    // 2. Did this callback come from the request we started?
    if q.state != csrf {
        return Err(AppError::BadRequest);
    }
    // 3. Trade the code for tokens
    let tokens = s
        .oidc
        .exchange_code(AuthorizationCode::new(q.code))
        .map_err(|e| AppError::Oidc(e.to_string()))?
        .set_pkce_verifier(PkceCodeVerifier::new(verifier))
        .request_async(&s.http)
        .await
        .map_err(|e| AppError::Oidc(e.to_string()))?;
    // 4. Verify the ID token's signature, issuer, audience, expiry and nonce
    let id_token = tokens.id_token().ok_or(AppError::BadRequest)?;
    let claims = id_token
        .claims(&s.oidc.id_token_verifier(), &Nonce::new(nonce))
        .map_err(|e| AppError::Oidc(e.to_string()))?;
    let name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.to_string())
        .unwrap_or_default();

    tracing::info!(sub = %claims.subject().as_str(), "login ok");

    // 5. Resolve the identity to a user, once. The session then outlives the id_token,
    //    which is good for about an hour and is not refreshed anywhere.
    let user = User::find_or_create(
        &s.ctx.db,
        user::Sub(claims.subject().to_string()),
        Some(name)
            .filter(|n: &String| !n.is_empty())
            .map(user::Name),
        claims.email().map(|e| user::Email(e.to_string())),
    )
    .await?;

    // 6. New session id, then store who they are
    session.cycle_id().await?;
    session.insert(auth::USER_ID, user.id.0).await?;
    Ok(Redirect::to("/"))
}

/// Builds the state this crate's routes need, discovering Google's OIDC endpoints.
///
/// Separate from [`router`] because discovery is a network call: the caller decides
/// when to pay for it, and gets a real error if it fails rather than a panic during
/// routing.
pub async fn state(ctx: Ctx) -> anyhow::Result<AppState> {
    let http = openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .build()?;

    let meta = CoreProviderMetadata::discover_async(
        IssuerUrl::new("https://accounts.google.com".to_string())?,
        &http,
    )
    .await?;

    let oidc = CoreClient::from_provider_metadata(
        meta,
        ClientId::new(std::env::var("GOOGLE_CLIENT_ID")?),
        Some(ClientSecret::new(std::env::var("GOOGLE_CLIENT_SECRET")?)),
    )
    .set_redirect_uri(RedirectUrl::new(
        std::env::var("REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8080/auth/callback".to_string()),
    )?);

    tracing::info!("discovered google oidc endpoints");

    Ok(AppState {
        oidc: Arc::new(oidc),
        http,
        ctx,
    })
}

/// Creates the session table, and returns the store to hand to [`router`].
///
/// Sessions live in the same SQLite file as everything else: one fewer thing to run,
/// and a restart no longer signs everybody out — which is what `MemoryStore` did on
/// every deploy and every `cargo run`.
pub async fn session_store(ctx: &Ctx) -> anyhow::Result<sessions::SqliteSessions> {
    let store = sessions::SqliteSessions::new(ctx);
    store.migrate().await?;
    Ok(store)
}

/// The browser-facing routes, with the session layer already applied.
///
/// The layer is attached here rather than by the caller so that it cannot be applied
/// to anything else by accident: a router that has been merged with the API's is no
/// longer safe to wrap in sessions, and this is the last point at which that is
/// still obvious.
pub fn router(state: AppState, sessions: sessions::SqliteSessions) -> Router {
    let session_layer = SessionManagerLayer::new(sessions)
        .with_secure(false)
        .with_http_only(true)
        // Lax keeps the cookie off cross-site non-navigation requests. It is depth
        // behind the real rule, not the rule itself: /api never reads cookies at all.
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(
            tower_sessions::cookie::time::Duration::days(7),
        ));

    Router::new()
        .route("/", get(pages::lists::index))
        .route("/lists", post(pages::lists::create))
        .route("/lists/{id}/delete", post(pages::lists::delete))
        .route("/lists/{id}", get(pages::items::show))
        .route("/lists/{id}/items", post(pages::items::create))
        // A browser form can only GET or POST, so ticking off and deleting are POSTs
        // to their own paths rather than PUT and DELETE on the item.
        .route(
            "/lists/{id}/items/{item_id}/toggle",
            post(pages::items::toggle),
        )
        .route(
            "/lists/{id}/items/{item_id}/delete",
            post(pages::items::delete),
        )
        .route(
            "/notes",
            get(pages::notes::index).post(pages::notes::create),
        )
        .route("/notes/{id}/delete", post(pages::notes::delete))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout))
        .layer(session_layer)
        .with_state(state)
}
