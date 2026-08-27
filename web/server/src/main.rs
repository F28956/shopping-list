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
use api::state::{AppState as ApiState, AuthMode, Provider};
use domain::service::Ctx;
use domain::service::admission::Admission;
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

    // One `Ctx`, cloned, rather than one built per transport. Cloning shares the
    // change notifier, and that sharing is the whole feature: a list edited in the
    // browser has to reach a phone watching it through the API. Two `Ctx::new` calls
    // would compile, pass every test, and silently never cross.
    let ctx = Ctx::new(db.clone());

    // `ALLOWED_EMAILS` is now a seed rather than a policy: it applies to a server that
    // has nothing stored yet, and is ignored for ever after. Optional, because on a
    // server that has been claimed there is nothing for it to do — see
    // `service::admission::seed`.
    domain::service::admission::seed(&ctx, admission()?.as_ref()).await?;

    // Offered only while nobody owns this server. On a claimed one there is no code,
    // so there is nothing to guess and nothing to leak in a log.
    let ctx = match offer_claim(&ctx).await? {
        Some(code) => ctx.awaiting_claim(code),
        None => ctx,
    };

    let api_state = ApiState {
        ctx: ctx.clone(),
        auth: AuthMode::Providers(providers()?),
    };
    let web_ctx = ctx.clone();
    let sessions = web::session_store(&web_ctx).await?;
    let web_state = web::state(web_ctx).await?;

    housekeeping(ctx.clone());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("listening on {}", listener.local_addr()?);

    // Said out loud because `localhost` is the first thing to get wrong once a real
    // phone is involved: on the handset it means the handset.
    if let Some(address) = lan_address(8080) {
        tracing::info!("reachable from this network at {address}");
    }
    axum::serve(listener, app(api_state, web_state, sessions)).await?;
    Ok(())
}

/// The code that claims an unclaimed server, printed where its operator is looking.
///
/// A2's answer to the land grab: between starting a process and somebody claiming it,
/// anybody who can reach the port would otherwise become the owner — and the person it
/// happens to gets no warning, they are simply refused from their own server.
///
/// A log line rather than an environment variable, because a self-hoster starts the
/// process and then opens the app, so they have the log in front of them; it works for
/// a packaged install where nobody is setting variables; and it is the only answer of
/// the three that is safe when the port is already public.
///
/// New on every restart, deliberately. It expires by the process ending, there is
/// nothing to store, and a code read off last month's log is not a key.
async fn offer_claim(ctx: &Ctx) -> anyhow::Result<Option<String>> {
    if domain::models::admission::Server::is_claimed(&ctx.db).await? {
        return Ok(None);
    }

    let code = domain::service::admission::new_claim_code();

    // Deliberately unstructured and loud. This is the one log line whose whole job is
    // to be read by a person, and a field buried in JSON is a field they will miss.
    tracing::warn!("");
    tracing::warn!("  Nobody owns this server yet.");
    tracing::warn!("  Claim it when you sign in, with the code:  {code}");
    tracing::warn!("");

    Ok(Some(code))
}

/// The retention this process owes, on a timer.
///
/// One task rather than one per thing to sweep, because "what does this server delete
/// and when" is a question with one answer and it should be readable in one place.
/// Today it is sessions; `item_history` needs nothing, being capped and trimmed on
/// write by `history::Entry::prune`.
///
/// Detached and never awaited. A failed sweep is logged and the next one tries again
/// — a database that cannot be written to is a problem the request path will report
/// far more loudly than this could, and stopping the server because a `DELETE` failed
/// would turn a tidiness problem into an outage.
fn housekeeping(ctx: Ctx) {
    /// Long, because nothing here is urgent: a session idle for ninety days can be
    /// idle for ninety days and six hours. Short enough that a server left running
    /// for a year does the work more than once.
    const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

    tokio::spawn(async move {
        loop {
            // Swept at boot as well as on the timer, so a server that is only ever
            // started, used and stopped still cleans up. Otherwise a machine
            // restarted daily would never reach the first interval.
            if let Err(e) = domain::service::sessions::sweep(&ctx).await {
                tracing::warn!(error = ?e, "sweeping sessions failed; will try again");
            }

            tokio::time::sleep(EVERY).await;
        }
    });
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

/// Who may sign in, from `ALLOWED_EMAILS`, if anybody said.
///
/// No longer required, and that is the change: admission is rows now, managed by
/// whoever owns the server, and this variable only ever seeds a database that has
/// none. A fresh install with nothing set is not misconfigured — it is unclaimed, and
/// the first person through the door claims it.
///
/// `ALLOWED_EMAILS="*"` still says "anyone may sign in", deliberately and in writing.
fn admission() -> anyhow::Result<Option<Admission>> {
    let Ok(configured) = std::env::var("ALLOWED_EMAILS") else {
        return Ok(None);
    };

    let admission = Admission::parse(&configured)?;
    match &admission {
        Admission::Anyone => tracing::warn!("ALLOWED_EMAILS is \"*\": anyone may sign in"),
        Admission::These(listed) => tracing::info!(configured = listed.len(), "admission seed"),
    }
    Ok(Some(admission))
}

/// The address a device on the same network can reach this on.
///
/// Found by asking the routing table, not by listing interfaces: connecting a UDP
/// socket sends no packets, it only resolves which local address would be used to
/// reach somewhere else — which is the one a phone on the same Wi-Fi can use.
///
/// `None` when there is no route out, which is a laptop with the Wi-Fi off rather
/// than an error worth stopping for.
fn lan_address(port: u16) -> Option<String> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("8.8.8.8:80").ok()?;
    Some(format!("http://{}:{port}", probe.local_addr().ok()?.ip()))
}

/// The Google client ids whose tokens this server will accept as its own.
///
/// One per way in, because Google decides the `aud` claim differently per platform:
///
/// * **Browser** — `GOOGLE_CLIENT_ID`, the web client. Required.
/// * **iOS** — `GOOGLE_IOS_CLIENT_ID`, and only for a build that still signs in with
///   Google. The Apple apps do not: they sign in with Apple and trade the result for
///   a session, so their audience is a bundle id in `APPLE_BUNDLE_IDS`.
/// * **Android** — usually *nothing to add*. Credential Manager is given the web
///   client id as its `serverClientId`, and the token comes back addressed to that,
///   already in this list. The Android OAuth client — registered against the package
///   name and the signing certificate's SHA-1 — exists so Google can attest the app,
///   not to name the audience. `GOOGLE_ANDROID_CLIENT_ID` is here for the case where
///   a token does arrive addressed to it, so that discovering as much is a line of
///   configuration rather than a change to the server.
/// Who this server will accept a token from.
///
/// Google is required: it is how the browser and Android sign in, and a server with no
/// Google audiences is one nobody can reach. Apple is optional, because it needs a paid
/// developer account and a bundle identifier, and a checkout without one should still
/// start rather than fail at boot with a message about a platform the person may not
/// own.
fn providers() -> anyhow::Result<Vec<Provider>> {
    let http = reqwest::Client::new();
    let mut providers = Vec::new();

    let google = audiences([
        Some(std::env::var("GOOGLE_CLIENT_ID")?),
        std::env::var("GOOGLE_IOS_CLIENT_ID").ok(),
        std::env::var("GOOGLE_ANDROID_CLIENT_ID").ok(),
    ]);
    tracing::info!(audiences = google.len(), "accepting Google tokens");
    providers.push(Provider {
        name: "google",
        jwks: Arc::new(Jwks::new(
            http.clone(),
            "https://www.googleapis.com/oauth2/v3/certs",
        )),
        // Spelled two ways historically, and Google still issues both.
        issuers: vec![
            "https://accounts.google.com".into(),
            "accounts.google.com".into(),
        ],
        audiences: google,
    });

    // The audience of a native Sign in with Apple token is the app's bundle
    // identifier -- there is no separate client id to configure, which is why this is
    // the bundle id and not something from a console.
    let apple = audiences([std::env::var("APPLE_BUNDLE_IDS").ok()].into_iter().flat_map(
        |configured| {
            configured
                .into_iter()
                .flat_map(|list| list.split(',').map(str::to_string).collect::<Vec<_>>())
                .map(Some)
                .collect::<Vec<_>>()
        },
    ));

    if apple.is_empty() {
        tracing::info!("not accepting Apple tokens: APPLE_BUNDLE_IDS is not set");
    } else {
        tracing::info!(audiences = apple.len(), "accepting Apple tokens");
        providers.push(Provider {
            name: "apple",
            jwks: Arc::new(Jwks::new(http, "https://appleid.apple.com/auth/keys")),
            issuers: vec!["https://appleid.apple.com".into()],
            audiences: apple,
        });
    }

    Ok(providers)
}

/// The configured ids, minus the blanks and the repeats.
///
/// Separated from reading the environment so the rules have somewhere to be tested.
/// Repeats are dropped rather than tolerated: Android is normally configured with the
/// web client id, and listing an audience twice is a validator doing the same work
/// twice and a log line that miscounts the ways in.
fn audiences(configured: impl IntoIterator<Item = Option<String>>) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();

    for value in configured.into_iter().flatten() {
        let id = value.trim().to_string();
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }

    ids
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
    /// The audiences, which decide whose tokens are ours.
    #[test]
    fn audiences_drop_blanks_and_repeats() {
        assert_eq!(
            super::audiences([
                Some("web".to_string()),
                Some("  ".to_string()),
                None,
                Some(" ios ".to_string()),
                // Android is normally configured with the web client id, because that
                // is what its tokens are addressed to.
                Some("web".to_string()),
            ]),
            vec!["web".to_string(), "ios".to_string()]
        );
    }

    #[test]
    fn one_way_in_is_enough() {
        assert_eq!(
            super::audiences([Some("web".to_string()), None, None]),
            vec!["web".to_string()]
        );
    }

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
