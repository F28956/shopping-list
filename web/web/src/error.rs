use axum::{
    http::StatusCode,
    response::{IntoResponse,Response},
};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request")]
    BadRequest,

    #[error(transparent)]
    Session(#[from] tower_sessions::session::Error),

    #[error("oidc: {0}")]
    Oidc(String)
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::BadRequest => StatusCode::BAD_REQUEST,
            AppError::Session(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::Oidc(_) => StatusCode::BAD_REQUEST,
        };
        tracing::error!(error = ?self, "web request failed");
        (status, "something went wrong").into_response()
    }
}
