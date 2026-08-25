//! Building a router in tests without touching the network.
//!
//! The only thing in this crate that needs the internet is OIDC discovery, and only
//! the two auth routes use the client it produces. The provider metadata can be built
//! by hand instead, which keeps the pages testable without making the production path
//! optional or fake.

use std::sync::Arc;

use openidconnect::core::{CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthUrl, ClientId, ClientSecret, EmptyAdditionalProviderMetadata, IssuerUrl, JsonWebKeySetUrl,
    RedirectUrl, ResponseTypes,
};

use crate::state::AppState;

/// Provider metadata with plausible endpoints and no discovery request.
pub fn offline_state(ctx: domain::service::Ctx) -> AppState {
    let issuer = IssuerUrl::new("https://accounts.google.com".into()).unwrap();
    let meta = CoreProviderMetadata::new(
        issuer,
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".into()).unwrap(),
        JsonWebKeySetUrl::new("https://www.googleapis.com/oauth2/v3/certs".into()).unwrap(),
        vec![ResponseTypes::new(vec![])],
        vec![],
        vec![],
        EmptyAdditionalProviderMetadata {},
    );

    let oidc = CoreClient::from_provider_metadata(
        meta,
        ClientId::new("test-client".into()),
        Some(ClientSecret::new("test-secret".into())),
    )
    .set_redirect_uri(RedirectUrl::new("http://localhost:8080/auth/callback".into()).unwrap());

    AppState {
        oidc: Arc::new(oidc),
        http: openidconnect::reqwest::ClientBuilder::new()
            .redirect(openidconnect::reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        ctx,
    }
}
