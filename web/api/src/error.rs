use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use domain::service::ServiceError;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,

    #[error("not found")]
    NotFound,

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error(transparent)]
    Service(#[from] ServiceError),
}

impl From<domain::models::Error> for AppError {
    /// Authentication touches a model directly — resolving the identity is what
    /// *produces* the actor, so it cannot itself take one.
    fn from(err: domain::models::Error) -> Self {
        AppError::Service(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Unauthorized | AppError::Jwt(_) => StatusCode::UNAUTHORIZED,
            AppError::NotFound => StatusCode::NOT_FOUND,
            // The service layer already decided whether a failure was the caller's
            // fault; collapsing these into a 500 would throw that away and page
            // someone for a duplicate name.
            AppError::Service(e) => match e {
                ServiceError::NotFound => StatusCode::NOT_FOUND,
                ServiceError::Conflict | ServiceError::InUse => StatusCode::CONFLICT,
                ServiceError::InvalidInput => StatusCode::BAD_REQUEST,
                ServiceError::Unauthenticated => StatusCode::UNAUTHORIZED,
                ServiceError::Forbidden => StatusCode::FORBIDDEN,
                ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            AppError::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = ?self, "request failed");
        }
        (status, Json(json!({ "error": status.canonical_reason()}))).into_response()
    }
}
