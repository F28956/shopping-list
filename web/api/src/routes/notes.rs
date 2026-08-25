use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use domain::models::OffsetPage;
use domain::models::note::{self, Body, Note};
use domain::service::notes;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::routes::PageQuery;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(read).put(update).delete(delete))
}

/// What a client sends. A DTO rather than the model's own newtype: `Body` is
/// deliberately not `Deserialize`, so nothing outside this crate can conjure one
/// without going through a route that normalises it.
#[derive(Debug, serde::Deserialize)]
pub struct NoteBody {
    pub body: String,
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<PageQuery<note::Field>>,
) -> Result<Json<OffsetPage<Note>>, AppError> {
    let page = notes::for_user(&state.ctx, &user.actor(), q.paging(), q.order_by()).await?;
    Ok(Json(page))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<NoteBody>,
) -> Result<(StatusCode, Json<Note>), AppError> {
    let note = notes::create(&state.ctx, &user.actor(), Body(input.body)).await?;
    Ok((StatusCode::CREATED, Json(note)))
}

async fn read(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<Note>, AppError> {
    let note = notes::get(&state.ctx, &user.actor(), note::Id(id)).await?;
    Ok(Json(note))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<NoteBody>,
) -> Result<Json<Note>, AppError> {
    let note = notes::update(&state.ctx, &user.actor(), note::Id(id), Body(input.body)).await?;
    Ok(Json(note))
}

async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    notes::delete(&state.ctx, &user.actor(), note::Id(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::body::Body as HttpBody;
    use axum::http::{Request, StatusCode};
    use domain::models::pool;
    use domain::service::Ctx;
    use http_body_util::BodyExt;
    use rstest::rstest;
    use serde_json::{Value, json};
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    use super::*;
    use crate::state::AuthMode;

    /// The real router, driven in-process. Once the browser stops going over HTTP,
    /// only iOS exercises this layer — so it has to be exercised here or it rots.
    fn app(pool: SqlitePool) -> Router {
        let state = AppState {
            ctx: Ctx::new(pool),
            auth: AuthMode::TrustTheToken,
        };
        Router::new().nest("/api/notes", router()).with_state(state)
    }

    /// In tests the bearer token is the subject, so this is "sign in as".
    fn as_user(sub: &str) -> String {
        format!("Bearer {sub}")
    }

    async fn send(app: &Router, req: Request<HttpBody>) -> (StatusCode, Value) {
        let res = app.clone().oneshot(req).await.expect("router panicked");
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, body)
    }

    fn get(uri: &str, auth: Option<&str>) -> Request<HttpBody> {
        let mut b = Request::builder().uri(uri).method("GET");
        if let Some(a) = auth {
            b = b.header("authorization", a);
        }
        b.body(HttpBody::empty()).unwrap()
    }

    fn json(method: &str, uri: &str, auth: &str, body: Value) -> Request<HttpBody> {
        Request::builder()
            .uri(uri)
            .method(method)
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(HttpBody::from(body.to_string()))
            .unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn a_note_round_trips(#[future(awt)] pool: SqlitePool) {
        let app = app(pool);
        let me = as_user("google-oauth2|me");

        let (status, created) = send(
            &app,
            json("POST", "/api/notes", &me, json!({"body": " buy milk "})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["body"], "buy milk", "normalised on the way in");
        let id = created["id"].as_i64().unwrap();

        let (status, read) = send(&app, get(&format!("/api/notes/{id}"), Some(&me))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read, created);

        let (status, page) = send(&app, get("/api/notes?order_by=id", Some(&me))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], 1);
        assert_eq!(page["items"][0]["id"], id);

        let (status, edited) = send(
            &app,
            json(
                "PUT",
                &format!("/api/notes/{id}"),
                &me,
                json!({"body": "oat milk"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(edited["body"], "oat milk");

        let (status, _) = send(
            &app,
            Request::builder()
                .uri(format!("/api/notes/{id}"))
                .method("DELETE")
                .header("authorization", &me)
                .body(HttpBody::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, _) = send(&app, get(&format!("/api/notes/{id}"), Some(&me))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    /// The service layer's authorization rule, seen from the wire: someone else's
    /// note is a 404, indistinguishable from one that never existed.
    #[rstest]
    #[tokio::test]
    async fn one_persons_note_is_invisible_to_another(#[future(awt)] pool: SqlitePool) {
        let app = app(pool);
        let mine = as_user("google-oauth2|owner");
        let theirs = as_user("google-oauth2|stranger");

        let (_, created) = send(
            &app,
            json("POST", "/api/notes", &mine, json!({"body": "private"})),
        )
        .await;
        let id = created["id"].as_i64().unwrap();

        for (method, body) in [
            ("GET", None),
            ("PUT", Some(json!({"body": "x"}))),
            ("DELETE", None),
        ] {
            let req = match body {
                Some(b) => json(method, &format!("/api/notes/{id}"), &theirs, b),
                None => Request::builder()
                    .uri(format!("/api/notes/{id}"))
                    .method(method)
                    .header("authorization", &theirs)
                    .body(HttpBody::empty())
                    .unwrap(),
            };
            let (status, _) = send(&app, req).await;
            assert_eq!(
                status,
                StatusCode::NOT_FOUND,
                "{method} leaked another user's note"
            );
        }

        let (_, page) = send(&app, get("/api/notes?order_by=id", Some(theirs.as_str()))).await;
        assert_eq!(page["total"], 0, "it must not appear in their list either");
    }

    #[rstest]
    #[tokio::test]
    async fn no_token_is_unauthorized(#[future(awt)] pool: SqlitePool) {
        let app = app(pool);

        let (status, _) = send(&app, get("/api/notes?order_by=id", None)).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// D2, from the wire: the API is bearer-only. A session cookie authenticates
    /// nothing here, which is what keeps `/api/*` from being CSRF-reachable once it
    /// shares an origin with the web UI.
    #[rstest]
    #[tokio::test]
    async fn a_session_cookie_authenticates_nothing(#[future(awt)] pool: SqlitePool) {
        let app = app(pool);

        let req = Request::builder()
            .uri("/api/notes?order_by=id")
            .method("GET")
            .header("cookie", "id=a-perfectly-valid-looking-session")
            .body(HttpBody::empty())
            .unwrap();
        let (status, _) = send(&app, req).await;

        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "a cookie must never authenticate an API route"
        );
    }

    #[rstest]
    #[case::empty_body(json!({"body": ""}), StatusCode::BAD_REQUEST)]
    #[case::whitespace_only(json!({"body": "   "}), StatusCode::BAD_REQUEST)]
    #[case::wrong_shape(json!({"note": "typo"}), StatusCode::UNPROCESSABLE_ENTITY)]
    #[tokio::test]
    async fn bad_input_is_a_client_error(
        #[future(awt)] pool: SqlitePool,
        #[case] body: Value,
        #[case] expected: StatusCode,
    ) {
        let app = app(pool);

        let (status, _) = send(&app, json("POST", "/api/notes", &as_user("github|1"), body)).await;

        assert_eq!(status, expected);
    }

    #[rstest]
    #[tokio::test]
    async fn a_missing_note_is_not_found(#[future(awt)] pool: SqlitePool) {
        let app = app(pool);

        let (status, _) = send(&app, get("/api/notes/9999", Some(&as_user("github|1")))).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
