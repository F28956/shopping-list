use axum::{
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use crate::base;
use domain::service::ServiceError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request")]
    BadRequest,

    /// Nobody is signed in. A browser gets sent to the login page rather than a
    /// status code — the API's 401 would be a dead end here.
    #[error("not signed in")]
    Unauthenticated,

    #[error(transparent)]
    Session(#[from] tower_sessions::session::Error),

    #[error("oidc: {0}")]
    Oidc(String),

    #[error(transparent)]
    Service(#[from] ServiceError),
}

impl From<domain::models::Error> for AppError {
    /// Signing someone in touches a model directly — resolving the identity is what
    /// *produces* the actor, so it cannot take one.
    fn from(err: domain::models::Error) -> Self {
        AppError::Service(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Not signed in is a redirect, not an error page: the person is one click
        // from fixing it and there is nothing to explain.
        if matches!(self, AppError::Unauthenticated)
            || matches!(self, AppError::Service(ServiceError::Unauthenticated))
        {
            return Redirect::to(&base::at("/auth/login")).into_response();
        }

        let status = match &self {
            AppError::BadRequest => StatusCode::BAD_REQUEST,
            AppError::Unauthenticated => StatusCode::UNAUTHORIZED,
            AppError::Session(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Oidc(_) => StatusCode::BAD_REQUEST,
            // The service layer already decided whose fault it was; a 500 for every
            // one of these would page someone for a typo in a form.
            AppError::Service(e) => match e {
                ServiceError::NotFound => StatusCode::NOT_FOUND,
                ServiceError::Conflict | ServiceError::InUse => StatusCode::CONFLICT,
                ServiceError::InvalidInput => StatusCode::BAD_REQUEST,
                ServiceError::Unauthenticated => StatusCode::UNAUTHORIZED,
                ServiceError::Forbidden | ServiceError::NotAdmitted => StatusCode::FORBIDDEN,
                ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
        };

        if status.is_server_error() {
            // Safe at `error` only because no variant here carries anything a person
            // typed: `Oidc` holds the provider's own message, `Session` holds the
            // store's. A variant that held a name or an address would put it in a log
            // that gets shipped elsewhere -- see `observability`'s module header.
            tracing::error!(error = ?self, "web request failed");
        } else {
            tracing::debug!(error = ?self, "web request rejected");
        }

        // The other door. `identity::from_session` re-checks admission on every
        // request, so a withdrawal shows up here on the very next page load -- which
        // is the property A4 exists for, and this is what makes it visible.
        if matches!(&self, AppError::Service(ServiceError::NotAdmitted)) {
            observability::instruments::admission_refused("session");
        }

        // Said plainly, because this one is not a fault and asking again will not
        // fix it. "Something went wrong" sends somebody to look for a problem that
        // is not there -- the server worked exactly as configured.
        let body = match &self {
            AppError::Service(ServiceError::NotAdmitted) => {
                "This account is not allowed to use this server. \
                 Ask whoever runs it to add your address."
            }
            _ => "something went wrong",
        };

        (status, body).into_response()
    }
}
