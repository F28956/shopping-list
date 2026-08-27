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
    /// Verify the token's signature, issuer, audience and expiry against the published
    /// keys of whichever provider issued it.
    ///
    /// A list, because there is more than one: the Apple clients sign in with Apple,
    /// and Android and the browser sign in with Google. A token is offered to each in
    /// turn and the first that accepts it decides who sent it — nothing reads a claim
    /// out of an unverified token to choose, because an unverified claim is whatever
    /// the sender felt like writing.
    Providers(Vec<Provider>),
    /// Tests only: the bearer token is taken to be the subject, unverified.
    ///
    /// Behind a feature rather than `#[cfg(test)]` so that other crates can drive
    /// this router in *their* tests — the composed router in `server` is where the
    /// cookie-versus-bearer boundary actually has to hold. The feature is enabled
    /// only from dev-dependencies, so a release build does not contain this variant.
    #[cfg(any(test, feature = "test-support"))]
    TrustTheToken,
}

/// One identity provider, and what a token from it has to say.
#[derive(Clone)]
pub struct Provider {
    /// What to call it in `user_identities`. Part of the key there, so it is a stable
    /// name rather than a display one.
    pub name: &'static str,
    pub jwks: Arc<Jwks>,
    /// Who may have issued it. Google is spelled two ways historically, which is why
    /// this is a list rather than a string.
    pub issuers: Vec<String>,
    /// Who it was minted for. Several, because one provider issues a different client
    /// id per platform: the browser's tokens carry the web client id, Android's the
    /// Android one, and Apple's carry the bundle identifier. All of them are this
    /// application, and a token minted for none of them is rejected — the list is
    /// what makes that check mean anything.
    pub audiences: Vec<String>,
}

#[derive(Clone)]
pub struct AppState {
    /// What service calls need. Holds the pool, so there is exactly one.
    pub ctx: Ctx,
    pub auth: AuthMode,
}
