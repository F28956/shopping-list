//! Numbers going out, by either of the two routes anybody asks for.
//!
//! **Push** (`METRICS_MODE=push`): OTLP over HTTP to a collector somebody runs, with
//! whatever headers that collector wants for authentication. The right answer where
//! the server sits behind a NAT and nothing can scrape it.
//!
//! **Pull** (`METRICS_MODE=pull`): a Prometheus-format endpoint on its own listener.
//! The right answer where a Prometheus already exists on the same network.
//!
//! Both at once is `both`, and it is not a strange thing to want: it is what a person
//! does while moving from one to the other.
//!
//! ## Why the scrape endpoint is not just a route on the application
//!
//! Mounting `/metrics` on the main router would have made it reachable by anything
//! that can reach the application, which on a self-hosted box is every device on the
//! house's Wi-Fi — the television, the doorbell, a guest's phone. `docs/self-hosting.md`
//! S8 does not treat the LAN as trusted, and it should not: the metrics say how many
//! lists exist, how many people signed in, when the household shops and when it stops.
//! That is exactly the metadata S10 spends its effort trying not to leak.
//!
//! The API's own bearer authentication is not the answer either. That token identifies
//! a *person*, and issuing one to a scraper would give a monitoring system read access
//! to everybody's lists in exchange for a counter.
//!
//! So: **a separate listener, on its own port, bound to loopback by default, with its
//! own token.** Three things rather than one, because each covers a different mistake.
//! Loopback by default means the accident — turning metrics on and forgetting about
//! them — is not reachable from the network at all, and a scraper on the same host or
//! a `docker` sidecar needs no secret. Moving the bind to a real interface is a
//! deliberate act, and it is **refused unless a token is set**, because that is the
//! moment the endpoint becomes reachable by the television. The separate port is what
//! lets somebody firewall it, and it means the endpoint shares no code path, no
//! session layer and no state with the application.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::get;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use sqlx::SqlitePool;

/// Which way the numbers leave, if they leave at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The default. Nothing is exported and no listener is opened — a shopping list
    /// for one household does not need to be monitored, and a default that opened a
    /// port would be a default nobody asked for.
    Off,
    /// A scrapable endpoint.
    Pull,
    /// OTLP to a collector.
    Push,
    Both,
}

impl Mode {
    fn scrapes(self) -> bool {
        matches!(self, Mode::Pull | Mode::Both)
    }

    fn pushes(self) -> bool {
        matches!(self, Mode::Push | Mode::Both)
    }

    fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Pull => "pull",
            Mode::Push => "push",
            Mode::Both => "both",
        }
    }
}

/// The scrape listener.
#[derive(Debug, Clone, PartialEq)]
pub struct Scrape {
    /// Loopback unless somebody said otherwise, and saying otherwise costs a token.
    pub bind: IpAddr,
    pub port: u16,
    /// The bearer a scraper must present. `None` is only reachable on loopback.
    pub token: Option<String>,
}

impl Scrape {
    /// Whether this address is reachable from somewhere other than this machine.
    ///
    /// `is_loopback` and not a comparison with `127.0.0.1`, so `::1` and the rest of
    /// `127/8` are covered — a rule that only knew one spelling of loopback would be a
    /// rule with a hole in it.
    fn reachable_from_the_network(&self) -> bool {
        !self.bind.is_loopback()
    }
}

/// Where the numbers are pushed.
#[derive(Debug, Clone, PartialEq)]
pub struct Otlp {
    pub endpoint: String,
    /// Whatever the collector wants for authentication. Names and values, never
    /// logged: this is a credential.
    pub headers: Vec<(String, String)>,
    pub interval: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    /// Present exactly when the mode scrapes.
    pub scrape: Option<Scrape>,
    /// Present exactly when the mode pushes.
    pub otlp: Option<Otlp>,
}

/// The shortest token worth calling one.
///
/// Refused rather than accepted-and-warned. A four-character token on an endpoint
/// bound to `0.0.0.0` is worse than no token, because it looks like protection on the
/// day somebody decides whether to firewall the port.
const TOKEN_MINIMUM: usize = 16;

/// The default scrape port. Not 9090, which is Prometheus's own, and not 8080, which
/// is the application's.
const DEFAULT_PORT: u16 = 9100;

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::read(|name| std::env::var(name).ok())
    }

    /// The same, over any source of values — the arrangement `tls::Settings::read`
    /// uses, and for the same reason: the rules are the interesting part and they
    /// should be testable without a process-wide environment.
    ///
    /// Everything that cannot work is refused here. A collector endpoint that is not a
    /// URL, a scrape port that collides with the application's, an exposed endpoint
    /// with no token — all of them are startup failures, because the alternative is a
    /// server that comes up looking healthy and is either silently not exporting or
    /// silently exporting to the whole house.
    pub fn read(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let mode = match get("METRICS_MODE").as_deref().map(str::trim) {
            None | Some("") | Some("off") => Mode::Off,
            Some("pull") => Mode::Pull,
            Some("push") => Mode::Push,
            Some("both") => Mode::Both,
            Some(other) => {
                anyhow::bail!("METRICS_MODE is \"{other}\"; it must be off, pull, push or both")
            }
        };

        let scrape = mode.scrapes().then(|| read_scrape(&get)).transpose()?;
        let otlp = mode.pushes().then(|| read_otlp(&get)).transpose()?;

        Ok(Settings { mode, scrape, otlp })
    }

    /// Said at startup, so a server exporting its numbers somewhere says so every time
    /// it starts. The endpoint is named; its headers are not.
    pub fn announce(&self) {
        if self.mode == Mode::Off {
            return;
        }
        tracing::info!(mode = self.mode.name(), "metrics");

        if let Some(scrape) = &self.scrape {
            tracing::info!(
                address = %format!("{}:{}", scrape.bind, scrape.port),
                guarded = scrape.token.is_some(),
                "serving prometheus metrics"
            );
            if scrape.reachable_from_the_network() {
                tracing::warn!(
                    "METRICS_BIND is not loopback: the scrape endpoint is reachable from \
                     the network, and only METRICS_TOKEN stands in front of it"
                );
            }
        }

        if let Some(otlp) = &self.otlp {
            tracing::info!(
                endpoint = %otlp.endpoint,
                headers = otlp.headers.len(),
                seconds = otlp.interval.as_secs(),
                "pushing metrics"
            );
        }
    }

    /// Builds the meter provider, installs it, and starts whatever listeners and
    /// timers the mode needs.
    ///
    /// Returns the provider so `main` can hold it: dropping an `SdkMeterProvider`
    /// shuts its readers down, so a provider that went out of scope here would produce
    /// a server that exports nothing and says nothing about why.
    pub fn install(&self, db: &SqlitePool) -> anyhow::Result<Option<SdkMeterProvider>> {
        if self.mode == Mode::Off {
            return Ok(None);
        }

        let mut builder = SdkMeterProvider::builder().with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("shopping-list")
                .build(),
        );

        // The registry the Prometheus text output is gathered from. Held by the route
        // rather than global, so the endpoint has exactly one source and there is no
        // ambient registry for anything else to register into.
        let registry = Registry::new();
        if self.scrape.is_some() {
            builder = builder.with_reader(
                opentelemetry_prometheus::exporter()
                    .with_registry(registry.clone())
                    .build()
                    .context("building the prometheus exporter")?,
            );
        }

        if let Some(otlp) = &self.otlp {
            let headers: HashMap<String, String> = otlp.headers.iter().cloned().collect();

            let exporter = opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .with_endpoint(&otlp.endpoint)
                .with_headers(headers)
                .build()
                .context("building the OTLP exporter")?;

            builder = builder.with_reader(
                opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
                    .with_interval(otlp.interval)
                    .build(),
            );
        }

        let provider = builder.build();
        opentelemetry::global::set_meter_provider(provider.clone());

        // Before anything serves, and this ordering is load-bearing: an instrument
        // built from the global provider *before* it is set is bound to the no-op one
        // for the life of the process, and the symptom is a scrape endpoint that
        // answers correctly and is always empty.
        observability::instruments::warm_up();

        watch_pool(db.clone());

        if let Some(scrape) = &self.scrape {
            serve_scrape(scrape.clone(), registry);
        }

        Ok(Some(provider))
    }
}

fn read_scrape(get: &impl Fn(&str) -> Option<String>) -> anyhow::Result<Scrape> {
    let bind: IpAddr = match get("METRICS_BIND").as_deref().map(str::trim) {
        None | Some("") => IpAddr::from([127, 0, 0, 1]),
        Some(address) => address
            .parse()
            .with_context(|| format!("METRICS_BIND is \"{address}\", which is not an address"))?,
    };

    let port: u16 = match get("METRICS_PORT").as_deref().map(str::trim) {
        None | Some("") => DEFAULT_PORT,
        Some(port) => port.parse().context("METRICS_PORT is not a number")?,
    };

    // Refused rather than discovered at bind time, where the message is "address
    // already in use" and says nothing about which of the two listeners lost.
    let app_port: u16 = get("PORT")
        .as_deref()
        .map(str::trim)
        .filter(|port| !port.is_empty())
        .map(str::parse)
        .transpose()
        .context("PORT is not a number")?
        .unwrap_or(8080);

    anyhow::ensure!(
        port != app_port,
        "METRICS_PORT is {port}, which is the port the application is on"
    );

    // `off` means there is no redirect listener, so there is nothing to collide with.
    if let Some(redirect) = get("HTTP_REDIRECT_PORT")
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "off")
    {
        let redirect: u16 = redirect.parse().context("HTTP_REDIRECT_PORT is not a number")?;
        anyhow::ensure!(
            port != redirect,
            "METRICS_PORT is {port}, which is the port the http redirect is on"
        );
    }

    let token = get("METRICS_TOKEN")
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty());

    if let Some(token) = &token {
        anyhow::ensure!(
            token.len() >= TOKEN_MINIMUM,
            "METRICS_TOKEN is {} characters; it must be at least {TOKEN_MINIMUM}",
            token.len()
        );
    }

    let scrape = Scrape { bind, port, token };

    // The one refusal this module exists for. Everything else here is a typo caught
    // early; this is the configuration that starts, works, and quietly serves the
    // household's metadata to anything on the network.
    anyhow::ensure!(
        !scrape.reachable_from_the_network() || scrape.token.is_some(),
        "METRICS_BIND is {bind}, which is reachable from the network, and METRICS_TOKEN \
         is not set. Either bind the scrape endpoint to loopback or give it a token."
    );

    Ok(scrape)
}

fn read_otlp(get: &impl Fn(&str) -> Option<String>) -> anyhow::Result<Otlp> {
    let endpoint = get("METRICS_OTLP_ENDPOINT")
        .map(|endpoint| endpoint.trim().to_string())
        .filter(|endpoint| !endpoint.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("METRICS_OTLP_ENDPOINT is required by this METRICS_MODE")
        })?;

    anyhow::ensure!(
        endpoint.starts_with("http://") || endpoint.starts_with("https://"),
        "METRICS_OTLP_ENDPOINT is \"{endpoint}\"; it must be an http:// or https:// URL"
    );

    // `name=value,name=value`. Split on the first `=` only, because a token is base64
    // and base64 ends in `=`.
    let mut headers = Vec::new();
    if let Some(configured) = get("METRICS_OTLP_HEADERS") {
        for pair in configured.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (name, value) = pair.split_once('=').ok_or_else(|| {
                // The pair is not quoted back. It is a credential, and a startup error
                // is written to the same log everything else is.
                anyhow::anyhow!(
                    "METRICS_OTLP_HEADERS holds an entry with no \"=\"; it is a \
                     comma-separated list of name=value"
                )
            })?;
            anyhow::ensure!(
                !name.trim().is_empty(),
                "METRICS_OTLP_HEADERS holds an entry with no name"
            );
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let interval = match get("METRICS_OTLP_INTERVAL_SECONDS").as_deref().map(str::trim) {
        None | Some("") => Duration::from_secs(60),
        Some(seconds) => {
            let seconds: u64 = seconds
                .parse()
                .context("METRICS_OTLP_INTERVAL_SECONDS is not a number")?;
            anyhow::ensure!(seconds > 0, "METRICS_OTLP_INTERVAL_SECONDS is 0");
            Duration::from_secs(seconds)
        }
    };

    Ok(Otlp { endpoint, headers, interval })
}

/// Samples the pool on a timer.
///
/// A timer rather than a hook, because the numbers worth having are "how many
/// connections are held right now" and there is no event for *right now*. Ten seconds
/// is short enough to see a pool exhausted by a slow write and long enough to cost
/// nothing.
fn watch_pool(db: SqlitePool) {
    const EVERY: Duration = Duration::from_secs(10);

    tokio::spawn(async move {
        loop {
            observability::instruments::db_pool(db.size(), db.num_idle());
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// The scrape listener: one route, its own port, and nothing else.
///
/// Detached, and a failure to bind is a warning rather than a stop — the same trade
/// the HTTP redirect listener makes. A server that cannot open its metrics port should
/// still serve the application; the person who wanted the numbers will see this line.
fn serve_scrape(scrape: Scrape, registry: Registry) {
    tokio::spawn(async move {
        let state = ScrapeState { token: scrape.token.clone(), registry };

        // No security headers, no session layer, no fallback. A scraper is not a
        // browser, and every layer this does not have is a layer that cannot be
        // wrong here.
        let app = Router::new().route("/metrics", get(render)).with_state(state);

        match tokio::net::TcpListener::bind((scrape.bind, scrape.port)).await {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::warn!(error = %e, "the metrics listener stopped");
                }
            }
            Err(e) => tracing::warn!(
                error = %e,
                "could not bind {}:{} for metrics; carrying on without them",
                scrape.bind,
                scrape.port
            ),
        }
    });
}

#[derive(Clone)]
struct ScrapeState {
    token: Option<String>,
    registry: Registry,
}

/// The Prometheus text body, for a caller that proved it may have it.
async fn render(State(state): State<ScrapeState>, headers: HeaderMap) -> (StatusCode, String) {
    if !authorised(&state.token, &headers) {
        // Not `WWW-Authenticate: Bearer` and no body. There is nothing here for an
        // unauthenticated caller to negotiate, and the shortest answer is the one that
        // says least about what is behind it.
        return (StatusCode::UNAUTHORIZED, String::new());
    }

    let mut body = Vec::new();
    match TextEncoder::new().encode(&state.registry.gather(), &mut body) {
        Ok(()) => (
            StatusCode::OK,
            String::from_utf8_lossy(&body).into_owned(),
        ),
        Err(e) => {
            tracing::warn!(error = %e, "could not encode metrics");
            (StatusCode::INTERNAL_SERVER_ERROR, String::new())
        }
    }
}

/// Whether this request may read the numbers.
///
/// `None` means no token was configured, which `read_scrape` only allows on loopback —
/// so "no token" is never the same thing as "no protection".
pub fn authorised(token: &Option<String>, headers: &HeaderMap) -> bool {
    let Some(expected) = token else {
        return true;
    };

    let Some(offered) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };

    constant_time_eq(offered.as_bytes(), expected.as_bytes())
}

/// Compares without leaking where the two differ.
///
/// Overkill for a scrape token and cheap enough not to argue about. `==` on byte
/// slices stops at the first difference, and an endpoint somebody can ask a thousand
/// times a second is exactly the shape a timing attack wants.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.iter()
        .zip(b)
        .fold(0u8, |differences, (x, y)| differences | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use rstest::rstest;

    use super::*;

    fn settings_from(vars: &[(&str, &str)]) -> anyhow::Result<Settings> {
        let vars: std::collections::HashMap<_, _> = vars.iter().copied().collect();
        Settings::read(|name| vars.get(name).map(|v| v.to_string()))
    }

    /// A shopping list for one household does not need to be monitored, so nothing is
    /// exported and no port is opened until somebody says so.
    #[test]
    fn nothing_configured_exports_nothing_and_opens_no_port() {
        let settings = settings_from(&[]).unwrap();

        assert_eq!(settings.mode, Mode::Off);
        assert_eq!(settings.scrape, None);
        assert_eq!(settings.otlp, None);
    }

    /// Turning the scrape endpoint on is not the same as exposing it. The default bind
    /// is loopback, which is why the default needs no token.
    #[test]
    fn pull_defaults_to_loopback_and_needs_no_secret_there() {
        let settings = settings_from(&[("METRICS_MODE", "pull")]).unwrap();
        let scrape = settings.scrape.expect("pull without a scrape listener");

        assert!(scrape.bind.is_loopback());
        assert_eq!(scrape.port, DEFAULT_PORT);
        assert_eq!(scrape.token, None);
        assert!(!scrape.reachable_from_the_network());
    }

    #[test]
    fn push_reads_its_collector_its_headers_and_its_interval() {
        let settings = settings_from(&[
            ("METRICS_MODE", "push"),
            ("METRICS_OTLP_ENDPOINT", "https://collector.example.com:4318/v1/metrics"),
            // A real token is base64 and ends in `=`, which is why the split is on the
            // first `=` and not on every one.
            ("METRICS_OTLP_HEADERS", "authorization=Bearer abc==, x-scope = tenant-4 "),
            ("METRICS_OTLP_INTERVAL_SECONDS", "15"),
        ])
        .unwrap();

        let otlp = settings.otlp.expect("push without a collector");
        assert_eq!(otlp.endpoint, "https://collector.example.com:4318/v1/metrics");
        assert_eq!(
            otlp.headers,
            vec![
                ("authorization".to_string(), "Bearer abc==".to_string()),
                ("x-scope".to_string(), "tenant-4".to_string()),
            ]
        );
        assert_eq!(otlp.interval, Duration::from_secs(15));
        assert_eq!(settings.scrape, None);
    }

    /// Both is what somebody has while they are moving from one to the other.
    #[test]
    fn both_opens_the_endpoint_and_pushes() {
        let settings = settings_from(&[
            ("METRICS_MODE", "both"),
            ("METRICS_OTLP_ENDPOINT", "http://localhost:4318/v1/metrics"),
        ])
        .unwrap();

        assert!(settings.scrape.is_some());
        assert!(settings.otlp.is_some());
    }

    /// Refused at startup rather than at the first scrape or the first export, either
    /// of which happens when nobody is watching.
    #[rstest]
    #[case::unknown_mode(&[("METRICS_MODE", "yes")])]
    #[case::push_with_no_collector(&[("METRICS_MODE", "push")])]
    #[case::both_with_no_collector(&[("METRICS_MODE", "both")])]
    #[case::a_collector_that_is_not_a_url(
        &[("METRICS_MODE", "push"), ("METRICS_OTLP_ENDPOINT", "collector.example.com")]
    )]
    #[case::a_header_with_no_name(
        &[("METRICS_MODE", "push"),
          ("METRICS_OTLP_ENDPOINT", "http://c:4318"),
          ("METRICS_OTLP_HEADERS", "=value")]
    )]
    #[case::a_header_that_is_not_a_pair(
        &[("METRICS_MODE", "push"),
          ("METRICS_OTLP_ENDPOINT", "http://c:4318"),
          ("METRICS_OTLP_HEADERS", "authorization")]
    )]
    #[case::an_interval_of_nothing(
        &[("METRICS_MODE", "push"),
          ("METRICS_OTLP_ENDPOINT", "http://c:4318"),
          ("METRICS_OTLP_INTERVAL_SECONDS", "0")]
    )]
    #[case::an_interval_that_is_not_a_number(
        &[("METRICS_MODE", "push"),
          ("METRICS_OTLP_ENDPOINT", "http://c:4318"),
          ("METRICS_OTLP_INTERVAL_SECONDS", "often")]
    )]
    #[case::a_bind_that_is_not_an_address(
        &[("METRICS_MODE", "pull"), ("METRICS_BIND", "everywhere")]
    )]
    #[case::a_port_that_is_not_a_number(&[("METRICS_MODE", "pull"), ("METRICS_PORT", "metrics")])]
    #[case::the_application_s_own_port(&[("METRICS_MODE", "pull"), ("METRICS_PORT", "8080")])]
    #[case::the_configured_application_port(
        &[("METRICS_MODE", "pull"), ("PORT", "9100"), ("METRICS_PORT", "9100")]
    )]
    #[case::the_redirect_port(
        &[("METRICS_MODE", "pull"), ("HTTP_REDIRECT_PORT", "9100"), ("METRICS_PORT", "9100")]
    )]
    #[case::a_token_too_short_to_be_one(
        &[("METRICS_MODE", "pull"), ("METRICS_BIND", "0.0.0.0"), ("METRICS_TOKEN", "hunter2")]
    )]
    fn configuration_that_cannot_work_is_refused(#[case] vars: &[(&str, &str)]) {
        assert!(settings_from(vars).is_err(), "{vars:?} was accepted");
    }

    /// The refusal this module exists for. An endpoint on a real interface with no
    /// token would start, work, and quietly serve the household's metadata to
    /// everything on the Wi-Fi.
    #[rstest]
    #[case::every_interface("0.0.0.0")]
    #[case::one_interface("192.168.1.10")]
    #[case::every_interface_v6("::")]
    fn an_exposed_scrape_endpoint_without_a_token_is_refused(#[case] bind: &str) {
        let refused = settings_from(&[("METRICS_MODE", "pull"), ("METRICS_BIND", bind)]);

        assert!(refused.is_err(), "{bind} was accepted with no token");
        assert!(
            format!("{:?}", refused.unwrap_err()).contains("METRICS_TOKEN"),
            "the refusal did not say what to do about it"
        );

        // And is accepted once there is one.
        assert!(
            settings_from(&[
                ("METRICS_MODE", "pull"),
                ("METRICS_BIND", bind),
                ("METRICS_TOKEN", "a-long-enough-secret"),
            ])
            .is_ok(),
            "{bind} was refused even with a token"
        );
    }

    /// Loopback is loopback however it is spelled. A rule that only knew `127.0.0.1`
    /// would let `::1` and the rest of `127/8` take the exposed path.
    #[rstest]
    #[case("127.0.0.1")]
    #[case("127.0.0.53")]
    #[case("::1")]
    fn loopback_is_recognised_in_every_spelling(#[case] bind: &str) {
        let settings =
            settings_from(&[("METRICS_MODE", "pull"), ("METRICS_BIND", bind)]).unwrap();

        assert!(!settings.scrape.unwrap().reachable_from_the_network());
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        headers
    }

    #[test]
    fn the_scrape_endpoint_answers_only_what_holds_the_token() {
        let expected = Some("a-long-enough-secret".to_string());

        assert!(authorised(&expected, &bearer("a-long-enough-secret")));
        assert!(!authorised(&expected, &bearer("a-long-enough-secreT")));
        assert!(!authorised(&expected, &bearer("")));
        assert!(!authorised(&expected, &HeaderMap::new()));

        // A scheme that is not Bearer is not a near miss, it is a different thing.
        let mut basic = HeaderMap::new();
        basic.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic c2VjcmV0"));
        assert!(!authorised(&expected, &basic));

        // No token configured is only reachable on loopback, where anything that can
        // ask is already on this machine.
        assert!(authorised(&None, &HeaderMap::new()));
    }

    /// End to end: an instrument recorded through `observability` comes out of the
    /// endpoint in Prometheus's format, labelled by route *pattern*.
    ///
    /// One test rather than several, because installing the global meter provider is a
    /// once-per-process act and a second test that did it would race this one.
    #[tokio::test]
    async fn the_endpoint_serves_what_was_recorded_labelled_by_pattern() {
        let registry = Registry::new();
        let exporter = opentelemetry_prometheus::exporter()
            .with_registry(registry.clone())
            .build()
            .unwrap();
        let provider = SdkMeterProvider::builder().with_reader(exporter).build();
        opentelemetry::global::set_meter_provider(provider.clone());
        observability::instruments::warm_up();

        observability::instruments()
            .request("/api/lists/{id}/items", "GET", 200, 0.012);
        observability::instruments::invite_redeemed(true);

        let state = ScrapeState { token: Some("a-long-enough-secret".into()), registry };

        let (status, body) = render(State(state.clone()), bearer("a-long-enough-secret")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("route=\"/api/lists/{id}/items\""),
            "the route pattern is not in the output:\n{body}"
        );
        assert!(
            body.contains("invites_redemptions"),
            "an ordinary counter is missing:\n{body}"
        );

        let (status, body) = render(State(state), bearer("wrong")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.is_empty(), "a refusal said something: {body}");
    }
}
