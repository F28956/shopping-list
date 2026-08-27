//! Serving HTTPS, or deliberately not.
//!
//! Both arrangements are supported and neither is assumed. Somebody who already runs
//! Caddy puts it in front and sets nothing; somebody with a bare box and one binary
//! gets a certificate without installing a second daemon. What decides which is
//! configuration, said out loud — see [`Mode`].
//!
//! TLS is a property of the listener and the router never learns about it (T2). Every
//! router test keeps running against a plain in-memory service, and there is no code
//! path where a handler behaves differently depending on how it was reached. The two
//! exceptions are deliberate and are both about telling the truth rather than changing
//! behaviour: the HSTS header (T10) and what `/healthz` says (T11).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use tokio::net::TcpListener;

/// How this process gets, or does not get, a certificate.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// Cleartext. The right answer behind a proxy that terminates TLS, and the default
    /// because a laptop talking to its own simulators must keep working with no
    /// configuration at all.
    Off,
    /// PEM files somebody else obtained — a corporate CA, a wildcard, `mkcert` on a
    /// development machine, or a certificate renewed by a tool that already exists.
    Files { cert: PathBuf, key: PathBuf },
    /// A certificate this process orders and renews itself.
    Acme {
        domains: Vec<String>,
        contact: Option<String>,
        staging: bool,
        cache: PathBuf,
    },
}

impl Mode {
    /// What to call it in a log line and in `/healthz`.
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Files { .. } => "files",
            Mode::Acme { .. } => "acme",
        }
    }

    /// Whether this process is the one holding a certificate.
    ///
    /// The question HSTS turns on. A server behind a terminating proxy is reached over
    /// HTTPS and does not know it, so it must not promise anything on the proxy's
    /// behalf; and one serving cleartext on a laptop must not send a header that locks
    /// a browser out of `http://localhost` for two years.
    pub fn serves_tls(&self) -> bool {
        !matches!(self, Mode::Off)
    }
}

/// Everything the listener needs to know.
#[derive(Debug, Clone)]
pub struct Settings {
    pub mode: Mode,
    pub port: u16,
    /// The plain-HTTP listener, which serves no application (T9). `None` where the
    /// operator said `off`, or where there is no TLS for it to redirect to.
    pub redirect_port: Option<u16>,
}

impl Settings {
    /// Reads the environment, refusing what cannot work rather than starting and
    /// failing later.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::read(|name| std::env::var(name).ok())
    }

    /// The same, over any source of values. Split out so the rules can be tested
    /// without a process-wide environment that other tests are also reading.
    pub fn read(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let required = |name: &str| {
            get(name).ok_or_else(|| anyhow::anyhow!("{name} is required by this TLS_MODE"))
        };

        let mode = match get("TLS_MODE").unwrap_or_else(|| "off".into()).as_str() {
            "off" => Mode::Off,
            "files" => Mode::Files {
                cert: required("TLS_CERT")?.into(),
                key: required("TLS_KEY")?.into(),
            },
            "acme" => {
                let domains: Vec<String> = required("TLS_DOMAINS")?
                    .split(',')
                    .map(|d| d.trim().to_lowercase())
                    .filter(|d| !d.is_empty())
                    .collect();

                anyhow::ensure!(!domains.is_empty(), "TLS_DOMAINS lists no names");

                // Refused here rather than by the CA, whose refusal arrives minutes
                // later and says something about an authorization object.
                for domain in &domains {
                    anyhow::ensure!(
                        domain.parse::<std::net::IpAddr>().is_err(),
                        "TLS_DOMAINS contains {domain}, which is an address and not a name — \
                         a public CA will not certify one"
                    );
                }

                Mode::Acme {
                    domains,
                    contact: get("ACME_CONTACT")
                        .map(|c| if c.contains(':') { c } else { format!("mailto:{c}") }),
                    // `production` by default, against the grain and on purpose: a
                    // staging certificate produces a server that starts cleanly,
                    // serves happily, and is rejected by every client — a failure that
                    // looks like a success everywhere except a browser's
                    // advanced-options dialog.
                    staging: get("ACME_DIRECTORY").as_deref() == Some("staging"),
                    cache: get("TLS_CACHE_DIR").unwrap_or_else(|| "./tls".into()).into(),
                }
            }
            other => anyhow::bail!("TLS_MODE is \"{other}\"; it must be off, files or acme"),
        };

        let port: u16 = match get("PORT") {
            Some(port) => port.parse().context("PORT is not a number")?,
            // 8080 stays the default even under TLS. A process that cannot bind 443
            // without help should not be told to try it by a default — see T12.
            None => 8080,
        };

        let redirect_port = match get("HTTP_REDIRECT_PORT").as_deref() {
            Some("off") => None,
            Some(port) => Some(port.parse::<u16>().context("HTTP_REDIRECT_PORT is not a number")?),
            // No redirect when there is nothing to redirect *to*: on a cleartext
            // server the plain listener is the server.
            None if !mode.serves_tls() => None,
            None => Some(80),
        };

        Ok(Settings { mode, port, redirect_port })
    }

    /// Said at startup in the same breath as `SESSION_INSECURE`, so a server serving
    /// cleartext says so every time it starts.
    pub fn announce(&self) {
        match &self.mode {
            Mode::Off => tracing::warn!(
                "TLS_MODE=off: serving cleartext. Correct behind a proxy that terminates \
                 TLS, and a leak anywhere else."
            ),
            Mode::Files { cert, .. } => {
                tracing::info!(certificate = %cert.display(), "serving TLS from files")
            }
            Mode::Acme { domains, staging, .. } => {
                if *staging {
                    tracing::warn!(
                        "ACME_DIRECTORY=staging: the certificate will be refused by every \
                         client. Right while you are fighting with port forwarding, and \
                         wrong afterwards."
                    );
                }
                tracing::info!(names = ?domains, "ordering and renewing a certificate");
            }
        }
    }
}

/// Chooses the cryptography rustls will use.
///
/// Required rather than tidy: two providers are in the dependency tree — `ring` here,
/// and whatever `reqwest` brings for outbound requests — and rustls refuses to guess
/// between them. Without this the process starts, binds, and panics inside the first
/// handshake, which is the worst moment to find out.
///
/// Ignores an error, which means somebody already installed one. That is a fine
/// outcome and not worth failing a boot over.
fn choose_cryptography() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Serves the application, wrapping the accepted socket when there is a certificate to
/// wrap it with.
pub async fn serve(mode: &Mode, listener: TcpListener, app: Router) -> anyhow::Result<()> {
    if mode.serves_tls() {
        choose_cryptography();
    }

    match mode {
        Mode::Off => Ok(axum::serve(listener, app).await?),
        Mode::Files { cert, key } => {
            let config = from_files(cert, key)?;
            serve_tls(listener, app, Arc::new(config)).await
        }
        Mode::Acme { .. } => {
            anyhow::bail!("TLS_MODE=acme is not built yet; use files, or off behind a proxy")
        }
    }
}

/// Reads a certificate chain and its key.
///
/// The whole chain, not only the leaf: a server that sends just its own certificate
/// works in a browser that happens to have cached the intermediate and fails on a
/// phone that has not, which is the most annoying shape a TLS problem can take.
fn from_files(cert: &PathBuf, key: &PathBuf) -> anyhow::Result<rustls::ServerConfig> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .with_context(|| format!("reading {}", cert.display()))?
        .collect::<Result<_, _>>()
        .with_context(|| format!("reading {}", cert.display()))?;

    anyhow::ensure!(!chain.is_empty(), "{} holds no certificate", cert.display());

    let private =
        PrivateKeyDer::from_pem_file(key).with_context(|| format!("reading {}", key.display()))?;

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, private)
        .context("the certificate and the key do not go together")?;

    // Advertised here because ALPN is negotiated during the handshake and there is no
    // later opportunity.
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(config)
}

/// The accept loop for a listener that terminates TLS.
///
/// A handshake that fails is logged at `debug` and dropped. It is not worth waking
/// anybody for: a port on the internet collects probes, and every one of them would
/// otherwise be a warning.
async fn serve_tls(
    listener: TcpListener,
    app: Router,
    config: Arc<rustls::ServerConfig>,
) -> anyhow::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder;
    use tokio_rustls::TlsAcceptor;
    use tower::Service;

    let acceptor = TlsAcceptor::from(config);

    loop {
        let (socket, peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let stream = match acceptor.accept(socket).await {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::debug!(%peer, error = %e, "TLS handshake failed");
                    return;
                }
            };

            let service = hyper::service::service_fn(move |request| {
                // Cloned per request, which is what axum's own `serve` does: the clone
                // is cheap and the borrow checker wants it.
                app.clone().call(request)
            });

            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(TokioIo::new(stream), service)
                .await
            {
                tracing::debug!(%peer, error = %e, "connection ended");
            }
        });
    }
}
