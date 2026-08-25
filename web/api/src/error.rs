use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use domain::models;
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
    Model(#[from] models::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Unauthorized | AppError::Jwt(_) => StatusCode::UNAUTHORIZED,
            AppError::NotFound => StatusCode::NOT_FOUND,
            // The model layer already decided whether a failure was the caller's
            // fault; collapsing these into a 500 would throw that away and page
            // someone for a duplicate name.
            AppError::Model(e) => match e {
                models::Error::NotFound => StatusCode::NOT_FOUND,
                models::Error::Conflict | models::Error::InUse => StatusCode::CONFLICT,
                models::Error::InvalidInput => StatusCode::BAD_REQUEST,
                models::Error::Database(_) | models::Error::System => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            },
            AppError::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = ?self, "request failed");
        }
        (status, Json(json!({ "error": status.canonical_reason()}))).into_response()
    }
}
