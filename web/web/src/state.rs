use std::sync::Arc;

use openidconnect::{core::CoreClient, EndpointMaybeSet, EndpointNotSet, EndpointSet};

pub type OidcClient = CoreClient<
    EndpointSet, // auth url - set by discovery
    EndpointNotSet, //device auth
    EndpointNotSet, // introspection
    EndpointNotSet, // revocation
    EndpointMaybeSet, // token url
    EndpointMaybeSet, // userinfo
>;

#[derive(Clone)]
pub struct AppState {
    pub oidc: Arc<OidcClient>,
    pub http: openidconnect::reqwest::Client,
    pub api_base: String,
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(serde::Deserialize)]
pub struct Note {
    pub id: i64,
    pub body: String,
}

#[derive(serde::Deserialize)]
pub struct NoteForm {
    pub body: String
}
