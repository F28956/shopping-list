use axum::{
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
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
            return Redirect::to("/auth/login").into_response();
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
                ServiceError::Forbidden => StatusCode::FORBIDDEN,
                ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
        };

        if status.is_server_error() {
            tracing::error!(error = ?self, "web request failed");
        } else {
            tracing::debug!(error = ?self, "web request rejected");
        }
        (status, "something went wrong").into_response()
    }
}
