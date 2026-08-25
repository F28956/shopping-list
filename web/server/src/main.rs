//! The executable. One process, one pool, one listener, three transports.
//!
//! This is the only crate with a `main`, and the only place that decides how the
//! routers are composed and which layers wrap which. That composition is a security
//! boundary, not plumbing — see [`app`].

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::{Router, routing::get};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use api::jwks::Jwks;
use api::state::{AppState as ApiState, AuthMode};
use domain::service::Ctx;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "server=debug,api=debug,web=debug,tower_http=debug,sqlx=warn".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let db = open_database().await?;
    domain::MIGRATOR.run(&db).await?;

    let api_state = ApiState {
        ctx: Ctx::new(db.clone()),
        auth: AuthMode::Google {
            jwks: Arc::new(Jwks::new(reqwest::Client::new())),
            client_id: std::env::var("GOOGLE_CLIENT_ID")?,
        },
    };
    let web_state = web::state(Ctx::new(db.clone())).await?;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app(api_state, web_state)).await?;
    Ok(())
}

/// Composes the transports onto one router.
///
/// The layering is the point:
///
/// * `web::router` arrives with its session layer already applied to its own routes,
///   and nothing else is ever wrapped in it. On a shared origin the browser attaches
///   the session cookie to `/api/*` as well, so a cookie that could authenticate an
///   API route would make every API route reachable from any site that links to it.
/// * `api::router` is nested under `/api` with no session layer at all. It
///   authenticates from `Authorization: Bearer` and nothing else.
/// * `CatchPanicLayer` wraps both, so a panic in one transport is a 500 on the
///   request that caused it rather than an outage for the others. That is why
///   `panic = "abort"` is deliberately absent from the release profile.
fn app(api_state: ApiState, web_state: web::AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api", api::router().with_state(api_state))
        .merge(web::router(web_state))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
}

/// Opens the database, deliberately refusing to create it.
///
/// `DATABASE_URL` is resolved against the current working directory unless it is
/// absolute, so creating on demand turns "wrong directory" into a silently empty
/// database rather than an error. Failing to open is the better symptom.
async fn open_database() -> anyhow::Result<SqlitePool> {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is not set");
    tracing::info!(%url, "opening database");

    let opts = SqliteConnectOptions::from_str(&url)?
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    Ok(SqlitePool::connect_with(opts).await?)
}
