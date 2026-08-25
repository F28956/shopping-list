mod auth;
mod error;
mod jwks;
mod state;

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use time::OffsetDateTime;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::jwks::Jwks;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "api=debug, tower_http=ddebug,sqlx=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is not set");
    tracing::info!("db url {}", db_url);
    // No create_if_missing. DATABASE_URL is a relative path resolved against the
    // current working directory, so launching from the wrong one used to mint a fresh
    // empty database and migrate it — leaving the app running happily against nothing.
    // Failing to open is a far better symptom than silently serving an empty database.
    // First-time setup: `cd web/api && sqlx database create && sqlx migrate run`.
    let opts = SqliteConnectOptions::from_str(&db_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    let db = SqlitePool::connect_with(opts).await?;
    domain::MIGRATOR.run(&db).await?;

    let state = AppState {
        db,
        jwks: Arc::new(Jwks::new(reqwest::Client::new())),
        google_client_id: std::env::var("GOOGLE_CLIENT_ID")?,
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/debug/jwks",
            get(|State(s): State<AppState>| async move {
                s.jwks.key("nonexistent").await.map(|_| "found")
            }),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("web listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
