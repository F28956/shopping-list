//! The server-rendered web UI: cookie-authenticated HTML, for browsers.
//!
//! Exports a router rather than serving one. The session layer is applied here, on
//! this crate's routes only — never to the combined router — so that a session cookie
//! can never authenticate an API route it happens to share an origin with.

use axum::{
    Form, Router,
    extract::{Query, State},
    response::Redirect,
    routing::{get, post},
};
use maud::{DOCTYPE, Markup, html};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use std::sync::Arc;
use tower_sessions::{Expiry, MemoryStore, Session, SessionManagerLayer, cookie::SameSite};

pub mod error;
pub use error::AppError;

pub mod state;
pub use state::{AppState, CallbackQuery, Note, NoteForm};

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

    // 5. New session ID then store identity
    session.cycle_id().await?;
    session.insert("id_token", id_token.to_string()).await?;
    session.insert("name", name).await?;
    Ok(Redirect::to("/"))
}

#[axum::debug_handler]
async fn index(session: Session, State(s): State<AppState>) -> Result<Markup, AppError> {
    let Some(token): Option<String> = session.get("id_token").await? else {
        return Ok(html! {
            (DOCTYPE)
            html {
                head {
                    title { "Shopping list" }
                }
                body {
                    h1 { "Shopping list" }
                    a href = "/auth/login" { "Sign in wih Google" }
                }
            }
        });
    };

    let name: String = session.get("name").await?.unwrap_or_default();

    let resp = s
        .http
        .get(format!("{}/api/notes", s.api_base))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| AppError::Oidc(e.to_string()))?;

    if resp.status() == 401 {
        session.flush().await?;
        return Ok(html! {
            meta http-equiv="refresh" content="0;url=/auth/login" {}
        });
    }

    let notes: Vec<Note> = resp
        .json()
        .await
        .map_err(|e| AppError::Oidc(e.to_string()))?;

    Ok(html! {
        (DOCTYPE)
        html {
            body {
                h1 {"Shopping list" }
                p { "Signed in as " (name) " - " a href="/auth/logout" { "sign out" } }
                form method="post" action="/notes" {
                    input type="text" name="body" placeholder="add an item" {}
                    button type="submit" { "Add" }
                }
                ul { @for n in &notes { li { (n.body) } } }
            }
        }
    })
}

async fn add_note(
    session: Session,
    State(s): State<AppState>,
    Form(form): Form<NoteForm>,
) -> Result<Redirect, AppError> {
    let token: String = session.get("id_token").await?.ok_or(AppError::BadRequest)?;

    s.http
        .post(format!("{}/api/notes", s.api_base))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "body": form.body }))
        .send()
        .await
        .map_err(|e| AppError::Oidc(e.to_string()))?;

    Ok(Redirect::to("/"))
}

/// Builds the state this crate's routes need, discovering Google's OIDC endpoints.
///
/// Separate from [`router`] because discovery is a network call: the caller decides
/// when to pay for it, and gets a real error if it fails rather than a panic during
/// routing.
pub async fn state() -> anyhow::Result<AppState> {
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
        // the API now lives in this same process, under /api
        api_base: std::env::var("API_BASE").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
    })
}

/// The browser-facing routes, with the session layer already applied.
///
/// The layer is attached here rather than by the caller so that it cannot be applied
/// to anything else by accident: a router that has been merged with the API's is no
/// longer safe to wrap in sessions, and this is the last point at which that is
/// still obvious.
pub fn router(state: AppState) -> Router {
    let session_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_secure(false)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(
            tower_sessions::cookie::time::Duration::days(7),
        ));

    Router::new()
        .route("/", get(index))
        .route("/notes", post(add_note))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", get(logout))
        .layer(session_layer)
        .with_state(state)
}
