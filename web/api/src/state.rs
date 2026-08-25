use std::sync::Arc;

use domain::service::Ctx;

use crate::jwks::Jwks;

/// How a bearer token becomes an identity.
///
/// An enum rather than a hard-wired call so the router can be driven in tests
/// without minting real Google tokens. The test variant is `#[cfg(test)]`, so it does
/// not exist in a release build — there is no flag that could turn it on in
/// production because there is no code for it to turn on.
#[derive(Clone)]
pub enum AuthMode {
    /// Verify the token's signature, issuer, audience and expiry against Google's
    /// published keys.
    Google { jwks: Arc<Jwks>, client_id: String },
    /// Tests only: the bearer token is taken to be the subject, unverified.
    #[cfg(test)]
    TrustTheToken,
}

#[derive(Clone)]
pub struct AppState {
    /// What service calls need. Holds the pool, so there is exactly one.
    pub ctx: Ctx,
    pub auth: AuthMode,
}
