use std::sync::Arc;

use openidconnect::{EndpointMaybeSet, EndpointNotSet, EndpointSet, core::CoreClient};

pub type OidcClient = CoreClient<
    EndpointSet,      // auth url - set by discovery
    EndpointNotSet,   //device auth
    EndpointNotSet,   // introspection
    EndpointNotSet,   // revocation
    EndpointMaybeSet, // token url
    EndpointMaybeSet, // userinfo
>;

#[derive(Clone)]
pub struct AppState {
    pub oidc: Arc<OidcClient>,
    /// Used for the OIDC token exchange only. The API is no longer reached over HTTP,
    /// so there is nothing else for it to call.
    pub http: openidconnect::reqwest::Client,
    /// What service calls need. The same pool the API uses, in the same process.
    pub ctx: domain::service::Ctx,
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(serde::Deserialize)]
pub struct NoteForm {
    pub body: String,
}
