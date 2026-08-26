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
                ServiceError::Forbidden | ServiceError::NotAdmitted => StatusCode::FORBIDDEN,
                ServiceError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            AppError::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = ?self, "request failed");
        }

        // A stable slug for the cases where the status alone is ambiguous. Two
        // different refusals share 403 -- "you may read this list but not change it"
        // and "this account cannot use this server at all" -- and a client that
        // cannot tell them apart ends up saying the first about the second. Absent
        // where the status says everything, so nothing has to be invented for the
        // ordinary cases and an older client reading only `error` sees no change.
        let reason = match &self {
            AppError::Service(ServiceError::NotAdmitted) => Some("not_admitted"),
            _ => None,
        };

        (
            status,
            Json(json!({ "error": status.canonical_reason(), "reason": reason })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_of(error: AppError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// Two refusals share 403, and `reason` is the only thing that tells them apart.
    /// A client that cannot will say "you may read this list but not change it" to
    /// somebody who has no list and no account -- which is what it used to do.
    #[tokio::test]
    async fn the_two_refusals_are_distinguishable() {
        let (admitted, refused) = body_of(AppError::Service(ServiceError::Forbidden)).await;
        let (not_admitted, stranger) =
            body_of(AppError::Service(ServiceError::NotAdmitted)).await;

        assert_eq!(admitted, StatusCode::FORBIDDEN);
        assert_eq!(not_admitted, StatusCode::FORBIDDEN);
        assert_eq!(refused["reason"], serde_json::Value::Null);
        assert_eq!(stranger["reason"], "not_admitted");
    }

    /// The field is present on every response, so a client may read it unconditionally
    /// rather than branching on whether the server bothered to send it.
    #[tokio::test]
    async fn ordinary_failures_carry_no_reason() {
        let (status, body) = body_of(AppError::NotFound).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "Not Found");
        assert!(body.get("reason").is_some(), "present, and null");
        assert_eq!(body["reason"], serde_json::Value::Null);
    }
}
