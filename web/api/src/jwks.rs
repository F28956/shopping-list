use jsonwebtoken::jwk::{Jwk, JwkSet};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

use super::error::AppError;

const TTL: Duration = Duration::from_secs(3600);
const MIN_REFRESH: Duration = Duration::from_secs(60);

struct Cached {
    keys: JwkSet,
    fetched_at: Option<Instant>,
    last_attempt: Option<Instant>,
}

pub struct Jwks {
    /// Where this provider publishes its signing keys.
    ///
    /// A field rather than a constant since there is more than one provider: Apple and
    /// Google each publish their own set, and a key from the wrong one is not a key
    /// that failed to verify — it is a key that was never asked about.
    certs_url: String,
    cache: RwLock<Cached>,
    refresh_lock: Mutex<()>,
    http: reqwest::Client,
}

impl Jwks {
    pub fn new(http: reqwest::Client, certs_url: impl Into<String>) -> Self {
        Self {
            certs_url: certs_url.into(),
            cache: RwLock::new(Cached {
                keys: JwkSet { keys: Vec::new() },
                fetched_at: None,
                last_attempt: None,
            }),
            refresh_lock: Mutex::new(()),
            http,
        }
    }
    pub async fn key(&self, kid: &str) -> Result<Jwk, AppError> {
        if let Some(k) = self.lookup_fresh(kid).await {
            return Ok(k);
        }

        self.refresh().await?;
        let c = self.cache.read().await;
        c.keys.find(kid).cloned().ok_or(AppError::Unauthorized)
    }

    async fn lookup_fresh(&self, kid: &str) -> Option<Jwk> {
        let c = self.cache.read().await;
        let fresh = c.fetched_at.is_some_and(|t| t.elapsed() < TTL);
        if fresh {
            c.keys.find(kid).cloned()
        } else {
            None
        }
    }

    async fn refresh(&self) -> Result<(), AppError> {
        let _guard = self.refresh_lock.lock().await;
        {
            let c = self.cache.read().await;

            // Someone refreshed while we queued on the mutex.
            if c.fetched_at.is_some_and(|t| t.elapsed() < MIN_REFRESH) {
                return Ok(());
            }

            //A recent attempt failed - don't hammer Google.
            if c.last_attempt.is_some_and(|t| t.elapsed() < MIN_REFRESH) {
                return Err(AppError::Unauthorized);
            }
        }

        self.cache.write().await.last_attempt = Some(Instant::now());
        let keys: JwkSet = self
            .http
            .get(&self.certs_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        tracing::info!(count = keys.keys.len(), "refreshed google jwks");

        let mut c = self.cache.write().await;
        c.keys = keys;
        c.fetched_at = Some(Instant::now());
        Ok(())
    }
}
