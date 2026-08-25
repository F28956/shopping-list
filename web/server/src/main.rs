//! The executable. One process, one pool, one listener, three transports.
//!
//! This is the only crate with a `main`, and the only place that decides how the
//! routers are composed and which layers wrap which. That composition is a security
//! boundary, not plumbing — see [`app`].

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderName, HeaderValue, header};
use axum::{Router, routing::get};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use tower::layer::util::Stack;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use api::jwks::Jwks;
use api::state::{AppState as ApiState, AuthMode};
use domain::service::Ctx;
use web::sessions::SqliteSessions;

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
            client_ids: google_client_ids()?,
        },
    };
    let web_ctx = Ctx::new(db.clone());
    let sessions = web::session_store(&web_ctx).await?;
    let web_state = web::state(web_ctx).await?;

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app(api_state, web_state, sessions)).await?;
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
fn app(api_state: ApiState, web_state: web::AppState, sessions: SqliteSessions) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api", api::router().with_state(api_state))
        .merge(web::router(web_state, sessions))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(security_headers())
}

/// The headers a browser needs to be told, since it assumes the worst otherwise.
///
/// The policy is strict on purpose, and the application was changed to fit it rather
/// than the other way round: the stylesheet and the two behaviours that were inline
/// `hx-on` attributes moved into served files, so `script-src` and `style-src` can
/// both say `self` and nothing else. A CSP that has to allow `unsafe-inline` is
/// mostly decoration.
type Header = SetResponseHeaderLayer<HeaderValue>;
type Headers = Stack<Header, Stack<Header, Stack<Header, Header>>>;

fn security_headers() -> Headers {
    const CSP: &str = "default-src 'self'; \
                       script-src 'self'; \
                       style-src 'self'; \
                       img-src 'self' data:; \
                       form-action 'self'; \
                       base-uri 'none'; \
                       frame-ancestors 'none'";

    Stack::new(
        SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        ),
        Stack::new(
            SetResponseHeaderLayer::overriding(
                HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            ),
            Stack::new(
                SetResponseHeaderLayer::overriding(
                    header::REFERRER_POLICY,
                    HeaderValue::from_static("same-origin"),
                ),
                // frame-ancestors above covers modern browsers; this covers the rest.
                SetResponseHeaderLayer::overriding(
                    HeaderName::from_static("x-frame-options"),
                    HeaderValue::from_static("DENY"),
                ),
            ),
        ),
    )
}

/// Every client id this application answers to.
///
/// One identity provider issues a different client id per platform, so the browser
/// and the phone present tokens with different audiences. `GOOGLE_CLIENT_ID` is
/// required — without it the web half cannot sign anybody in — and
/// `GOOGLE_IOS_CLIENT_ID` is added when the phone app has been set up.
fn google_client_ids() -> anyhow::Result<Vec<String>> {
    let mut ids = vec![std::env::var("GOOGLE_CLIENT_ID")?];

    if let Ok(ios) = std::env::var("GOOGLE_IOS_CLIENT_ID")
        && !ios.trim().is_empty()
    {
        ids.push(ios);
    }

    tracing::info!(audiences = ids.len(), "accepting Google tokens");
    Ok(ids)
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use domain::models::pool;
    use http_body_util::BodyExt;
    use rstest::rstest;
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    use super::*;

    /// The real composed router — web routes, API nested under /api, both layers on.
    ///
    /// Building it here rather than testing the two routers separately is the whole
    /// point: the boundary this exercises only exists once they share an origin.
    async fn composed(pool: SqlitePool) -> Router {
        let ctx = Ctx::new(pool);
        let api_state = ApiState {
            ctx: ctx.clone(),
            auth: AuthMode::TrustTheToken,
        };
        let sessions = web::session_store(&ctx).await.expect("session table");
        // The OIDC discovery call is a network round trip, so the web half is
        // represented by its session layer alone. That is the half this test is about.
        Router::new()
            .nest("/api", api::router().with_state(api_state))
            .layer(tower_sessions::SessionManagerLayer::new(sessions))
            .layer(CatchPanicLayer::new())
    }

    async fn status(app: &Router, req: Request<Body>) -> StatusCode {
        let res = app.clone().oneshot(req).await.expect("router panicked");
        let status = res.status();
        let _ = res.into_body().collect().await;
        status
    }

    /// D2. On a shared origin the browser attaches its session cookie to /api/* as
    /// well, so if a cookie could authenticate an API route, every API route would be
    /// reachable from any site that links to it. The API reads bearer tokens and
    /// nothing else.
    ///
    /// This is deliberately tested through the *composed* router: mounting an API
    /// route under the session layer is a one-line mistake with no visible symptom —
    /// the endpoint keeps working, it just also works for everyone else.
    #[rstest]
    #[case::a_plausible_session("id=abcdefghijklmnopqrstuvwxyz123456")]
    #[case::several_cookies("other=1; id=abcdefghijklmnopqrstuvwxyz123456; theme=dark")]
    #[tokio::test]
    async fn a_cookie_never_authenticates_the_api(
        #[future(awt)] pool: SqlitePool,
        #[case] cookie: &str,
    ) {
        let app = composed(pool).await;

        let req = Request::builder()
            .uri("/api/notes?order_by=id")
            .method("GET")
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap();

        assert_eq!(
            status(&app, req).await,
            StatusCode::UNAUTHORIZED,
            "a session cookie authenticated an API route"
        );
    }

    /// The other half of the same rule: a bearer token still works when the session
    /// layer is in the stack, so the boundary is not simply rejecting everything.
    #[rstest]
    #[tokio::test]
    async fn a_bearer_token_still_works_alongside_the_session_layer(
        #[future(awt)] pool: SqlitePool,
    ) {
        let app = composed(pool).await;

        let req = Request::builder()
            .uri("/api/notes?order_by=id")
            .method("GET")
            .header("authorization", "Bearer google-oauth2|someone")
            .body(Body::empty())
            .unwrap();

        assert_eq!(status(&app, req).await, StatusCode::OK);
    }

    /// A cookie alongside a valid bearer token must not change the answer either —
    /// the cookie is simply not consulted.
    #[rstest]
    #[tokio::test]
    async fn a_cookie_is_ignored_when_a_token_is_present(#[future(awt)] pool: SqlitePool) {
        let app = composed(pool).await;

        let req = Request::builder()
            .uri("/api/notes?order_by=id")
            .method("GET")
            .header("authorization", "Bearer google-oauth2|someone")
            .header("cookie", "id=abcdefghijklmnopqrstuvwxyz123456")
            .body(Body::empty())
            .unwrap();

        assert_eq!(status(&app, req).await, StatusCode::OK);
    }
}

#[cfg(test)]
mod security_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use domain::models::pool;
    use http_body_util::BodyExt;
    use rstest::rstest;
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    use super::*;

    /// The headers are set on the composed router, so they are asserted there: a layer
    /// applied to one half only would still look right in that half's own tests.
    #[rstest]
    #[case::csp("content-security-policy")]
    #[case::nosniff("x-content-type-options")]
    #[case::referrer("referrer-policy")]
    #[case::framing("x-frame-options")]
    #[tokio::test]
    async fn every_response_carries_its_security_headers(
        #[future(awt)] pool: SqlitePool,
        #[case] name: &str,
    ) {
        let ctx = Ctx::new(pool);
        let api_state = ApiState {
            ctx: ctx.clone(),
            auth: AuthMode::TrustTheToken,
        };
        let app = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .nest("/api", api::router().with_state(api_state))
            .layer(security_headers());

        for uri in ["/healthz", "/api/notes?order_by=id"] {
            let res = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert!(
                res.headers().contains_key(name),
                "{uri} came back without {name}"
            );
            let _ = res.into_body().collect().await;
        }
    }

    /// A policy is only worth sending if it forbids inline script and style. One that
    /// allows them is decoration.
    #[rstest]
    #[tokio::test]
    async fn the_policy_forbids_inline_code(#[future(awt)] pool: SqlitePool) {
        let _ = pool;
        let app = Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .layer(security_headers());

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let csp = res
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("no policy")
            .to_str()
            .unwrap()
            .to_string();

        assert!(
            !csp.contains("unsafe-inline"),
            "the policy allows inline code: {csp}"
        );
        assert!(
            !csp.contains("unsafe-eval"),
            "the policy allows eval: {csp}"
        );
        assert!(
            csp.contains("frame-ancestors 'none'"),
            "framing is not forbidden: {csp}"
        );
        assert!(
            csp.contains("base-uri 'none'"),
            "a <base> could rewrite every URL on the page: {csp}"
        );
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// The safe answer is the one you get by not thinking about it.
    #[test]
    fn the_session_cookie_is_secure_unless_told_otherwise() {
        // SAFETY: nothing else in this test binary reads this variable, and it is
        // removed again before the test ends.
        unsafe { std::env::remove_var("SESSION_INSECURE") };
        assert!(
            web::cookie_secure(),
            "an unset environment must mean secure"
        );

        unsafe { std::env::set_var("SESSION_INSECURE", "true") };
        assert!(
            !web::cookie_secure(),
            "the local-development exception does not work"
        );

        unsafe { std::env::set_var("SESSION_INSECURE", "no") };
        assert!(
            web::cookie_secure(),
            "only an explicit true may switch it off"
        );

        unsafe { std::env::remove_var("SESSION_INSECURE") };
    }
}
