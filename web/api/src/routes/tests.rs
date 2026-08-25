//! Request-level tests for the list and item routes.
//!
//! These exist because of D6: once the browser stops going over HTTP, only iOS
//! exercises this layer, and a layer nobody watches rots. They drive the real router
//! in-process, so serialisation, status codes and the nesting all get checked.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use domain::models::{fixtures, pool};
use domain::service::Ctx;
use http_body_util::BodyExt;
use rstest::rstest;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tower::ServiceExt;

use crate::state::{AppState, AuthMode};

fn app(pool: SqlitePool) -> Router {
    let state = AppState {
        ctx: Ctx::new(pool),
        auth: AuthMode::TrustTheToken,
    };
    Router::new()
        .nest("/api", crate::router())
        .with_state(state)
}

/// In tests the bearer token is the subject, so this is "sign in as".
fn me() -> String {
    "Bearer google-oauth2|me".to_string()
}

fn them() -> String {
    "Bearer google-oauth2|someone-else".to_string()
}

async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
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

fn req(method: &str, uri: &str, auth: &str, body: Option<Value>) -> Request<Body> {
    let b = Request::builder()
        .uri(uri)
        .method(method)
        .header("authorization", auth);
    match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

/// A list belonging to `me()`, with one item on it.
async fn a_list_with_an_item(app: &Router) -> (i64, i64) {
    let (status, list) = send(
        app,
        req(
            "POST",
            "/api/lists",
            &me(),
            Some(json!({"name": "Fruit & veg"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let list_id = list["id"].as_i64().unwrap();

    let (status, item) = send(
        app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items"),
            &me(),
            Some(json!({"name": "Apples", "amount": 2.0})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    (list_id, item["id"].as_i64().unwrap())
}

#[rstest]
#[tokio::test]
async fn a_list_round_trips(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (_, created) = send(
        &app,
        req(
            "POST",
            "/api/lists",
            &me(),
            Some(json!({"name": "  Dairy "})),
        ),
    )
    .await;
    assert_eq!(created["name"], "Dairy", "trimmed on the way in");
    let id = created["id"].as_i64().unwrap();

    let (status, page) = send(&app, req("GET", "/api/lists?order_by=id", &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1);

    let (status, renamed) = send(
        &app,
        req(
            "PUT",
            &format!("/api/lists/{id}"),
            &me(),
            Some(json!({"name": "Dairy & eggs"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "Dairy & eggs");

    let (status, _) = send(
        &app,
        req("DELETE", &format!("/api/lists/{id}"), &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&app, req("GET", &format!("/api/lists/{id}"), &me(), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[rstest]
#[tokio::test]
async fn an_item_round_trips_and_ticks_off(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;
    let base = format!("/api/lists/{list_id}/items/{item_id}");

    let (status, item) = send(&app, req("GET", &base, &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(item["amount"], 2.0);
    assert!(item["done_at"].is_null(), "a new item is outstanding");

    let (status, ticked) = send(&app, req("POST", &format!("{base}/done"), &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!ticked["done_at"].is_null(), "ticking stamps done_at");

    let (status, unticked) = send(&app, req("DELETE", &format!("{base}/done"), &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(unticked["done_at"].is_null(), "unticking clears it");

    let (status, _) = send(&app, req("DELETE", &base, &me(), None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, page) = send(
        &app,
        req(
            "GET",
            &format!("/api/lists/{list_id}/items?order_by=id"),
            &me(),
            None,
        ),
    )
    .await;
    assert_eq!(page["total"], 0);
}

/// The service layer's rule, seen from the wire. An item id is not a capability:
/// the list is what gets consulted, so knowing the id buys nothing.
#[rstest]
#[tokio::test]
async fn another_persons_list_and_items_are_invisible(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;

    for (method, uri, body) in [
        ("GET", format!("/api/lists/{list_id}"), None),
        (
            "PUT",
            format!("/api/lists/{list_id}"),
            Some(json!({"name": "mine now"})),
        ),
        ("DELETE", format!("/api/lists/{list_id}"), None),
        (
            "GET",
            format!("/api/lists/{list_id}/items?order_by=id"),
            None,
        ),
        (
            "POST",
            format!("/api/lists/{list_id}/items"),
            Some(json!({"name": "smuggled"})),
        ),
        ("DELETE", format!("/api/lists/{list_id}/items/done"), None),
        ("GET", format!("/api/lists/{list_id}/items/{item_id}"), None),
        (
            "PUT",
            format!("/api/lists/{list_id}/items/{item_id}"),
            Some(json!({"name": "theirs now", "amount": 99.0})),
        ),
        (
            "POST",
            format!("/api/lists/{list_id}/items/{item_id}/done"),
            None,
        ),
        (
            "DELETE",
            format!("/api/lists/{list_id}/items/{item_id}"),
            None,
        ),
    ] {
        let (status, _) = send(&app, req(method, &uri, &them(), body)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri} leaked");
    }

    // and the owner's list is exactly as it was
    let (_, page) = send(
        &app,
        req(
            "GET",
            &format!("/api/lists/{list_id}/items?order_by=id"),
            &me(),
            None,
        ),
    )
    .await;
    assert_eq!(page["total"], 1, "nothing was added or removed");
    // status codes alone would not notice a write that happened anyway
    assert_eq!(
        page["items"][0]["name"], "Apples",
        "a stranger edited the item"
    );
    assert_eq!(
        page["items"][0]["amount"], 2.0,
        "a stranger changed the amount"
    );
}

/// Reference data is readable by anyone signed in, and writable by nobody: the write
/// routes do not exist rather than existing and always refusing.
#[rstest]
#[case::units("/api/units")]
#[case::tags("/api/tags")]
#[tokio::test]
async fn reference_data_is_read_only(#[future(awt)] pool: SqlitePool, #[case] path: &str) {
    let app = app(pool);

    let (status, _) = send(
        &app,
        req("GET", &format!("{path}?order_by=id"), &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
        req("POST", path, &me(), Some(json!({"name": "mine"}))),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::METHOD_NOT_ALLOWED,
        "a write route exists where none should"
    );
}

#[rstest]
#[case::no_name(json!({}), StatusCode::UNPROCESSABLE_ENTITY)]
#[case::empty_name(json!({"name": ""}), StatusCode::BAD_REQUEST)]
#[case::whitespace_name(json!({"name": "   "}), StatusCode::BAD_REQUEST)]
#[tokio::test]
async fn bad_list_input_is_a_client_error(
    #[future(awt)] pool: SqlitePool,
    #[case] body: Value,
    #[case] expected: StatusCode,
) {
    let app = app(pool);

    let (status, _) = send(&app, req("POST", "/api/lists", &me(), Some(body))).await;

    assert_eq!(status, expected);
}

/// `CHECK (amount > 0)`, reaching the client as a 400 rather than a 500.
#[rstest]
#[case::zero(0.0)]
#[case::negative(-1.0)]
#[tokio::test]
async fn a_non_positive_amount_is_rejected(#[future(awt)] pool: SqlitePool, #[case] amount: f64) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (status, _) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items"),
            &me(),
            Some(json!({"name": "Apples", "amount": amount})),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// A unit that does not exist is the caller's mistake, not a server fault.
#[rstest]
#[tokio::test]
async fn an_unknown_unit_is_a_client_error(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (status, _) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items"),
            &me(),
            Some(json!({"name": "Apples", "unit_id": 9999})),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Tags reach an item through its list, so the same 404 rule applies: a tag id and an
/// item id together buy nothing if the list is not yours.
#[rstest]
#[tokio::test]
async fn tagging_an_item(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;
    let base = format!("/api/lists/{list_id}/items/{item_id}/tags");

    // pick a real tag from the seeded reference data
    let (_, tags) = send(&app, req("GET", "/api/tags?order_by=name", &me(), None)).await;
    let tag_id = tags["items"][0]["id"].as_i64().expect("no seeded tags");

    let (status, _) = send(&app, req("GET", &base, &me(), None)).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
        req("POST", &base, &me(), Some(json!({"tag_id": tag_id}))),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, on_item) = send(&app, req("GET", &base, &me(), None)).await;
    assert_eq!(on_item.as_array().unwrap().len(), 1);
    assert_eq!(on_item[0]["id"], tag_id);

    // attaching the same one twice is a conflict, not a silent second row
    let (status, _) = send(
        &app,
        req("POST", &base, &me(), Some(json!({"tag_id": tag_id}))),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = send(
        &app,
        req("DELETE", &format!("{base}/{tag_id}"), &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, on_item) = send(&app, req("GET", &base, &me(), None)).await;
    assert!(on_item.as_array().unwrap().is_empty());

    // detaching one that is not attached is a miss
    let (status, _) = send(
        &app,
        req("DELETE", &format!("{base}/{tag_id}"), &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[rstest]
#[tokio::test]
async fn a_stranger_cannot_tag_my_item(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;
    let base = format!("/api/lists/{list_id}/items/{item_id}/tags");
    let (_, tags) = send(&app, req("GET", "/api/tags?order_by=name", &me(), None)).await;
    let tag_id = tags["items"][0]["id"].as_i64().unwrap();

    for (method, uri, body) in [
        ("GET", base.clone(), None),
        ("POST", base.clone(), Some(json!({"tag_id": tag_id}))),
        ("DELETE", format!("{base}/{tag_id}"), None),
    ] {
        let (status, _) = send(&app, req(method, &uri, &them(), body)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri} leaked");
    }
}

/// A tag that does not exist is the caller's mistake, not a server fault.
#[rstest]
#[tokio::test]
async fn attaching_an_unknown_tag_is_a_client_error(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;

    let (status, _) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items/{item_id}/tags"),
            &me(),
            Some(json!({"tag_id": 9999})),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// PUT on an item replaces what a person typed, and leaves the rest of the row alone.
#[rstest]
#[tokio::test]
async fn editing_an_item(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;
    let base = format!("/api/lists/{list_id}/items/{item_id}");

    // tick it off first, so we can see whether editing disturbs that
    send(&app, req("POST", &format!("{base}/done"), &me(), None)).await;

    let (status, edited) = send(
        &app,
        req(
            "PUT",
            &base,
            &me(),
            Some(json!({"name": "  Braeburn apples ", "amount": 1.5})),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(edited["name"], "Braeburn apples", "trimmed on the way in");
    assert_eq!(
        edited["amount"], 1.5,
        "a REAL amount survives the round trip"
    );
    assert!(
        !edited["done_at"].is_null(),
        "editing must not un-tick the item"
    );
    assert_eq!(edited["list_id"], list_id, "an edit is not a move");
}

/// Clearing takes the ticked-off rows and nothing else.
#[rstest]
#[tokio::test]
async fn clearing_done_items(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, done_id) = a_list_with_an_item(&app).await;
    let items = format!("/api/lists/{list_id}/items");

    // a second row, left outstanding, so we can see what clearing spares
    let (status, kept) = send(
        &app,
        req("POST", &items, &me(), Some(json!({"name": "Pears"}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let kept_id = kept["id"].as_i64().unwrap();

    send(
        &app,
        req("POST", &format!("{items}/{done_id}/done"), &me(), None),
    )
    .await;

    let (status, cleared) = send(&app, req("DELETE", &format!("{items}/done"), &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["cleared"], 1, "one row was ticked off");

    let (_, page) = send(&app, req("GET", &format!("{items}?order_by=id"), &me(), None)).await;
    let left: Vec<i64> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect();
    assert_eq!(left, vec![kept_id], "the outstanding row is untouched");

    // Nothing ticked off is not an error -- the button is there either way.
    let (status, cleared) = send(&app, req("DELETE", &format!("{items}/done"), &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["cleared"], 0);
}

/// `done` is a route, not an item id: a viewer must not get past it either.
#[rstest]
#[tokio::test]
async fn a_stranger_cannot_clear_my_done_items(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;
    send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items/{item_id}/done"),
            &me(),
            None,
        ),
    )
    .await;

    let (status, _) = send(
        &app,
        req(
            "DELETE",
            &format!("/api/lists/{list_id}/items/done"),
            &them(),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a hidden list stays hidden");

    let (_, page) = send(
        &app,
        req(
            "GET",
            &format!("/api/lists/{list_id}/items?order_by=id"),
            &me(),
            None,
        ),
    )
    .await;
    assert_eq!(page["total"], 1, "the row is still there");
}

#[rstest]
#[case::empty_name(json!({"name": "  "}), StatusCode::BAD_REQUEST)]
#[case::zero_amount(json!({"name": "Apples", "amount": 0}), StatusCode::BAD_REQUEST)]
#[case::unknown_unit(json!({"name": "Apples", "unit_id": 9999}), StatusCode::BAD_REQUEST)]
#[tokio::test]
async fn bad_item_edits_are_client_errors(
    #[future(awt)] pool: SqlitePool,
    #[case] body: Value,
    #[case] expected: StatusCode,
) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;

    let (status, _) = send(
        &app,
        req(
            "PUT",
            &format!("/api/lists/{list_id}/items/{item_id}"),
            &me(),
            Some(body),
        ),
    )
    .await;

    assert_eq!(status, expected);

    // the item is untouched
    let (_, item) = send(
        &app,
        req(
            "GET",
            &format!("/api/lists/{list_id}/items/{item_id}"),
            &me(),
            None,
        ),
    )
    .await;
    assert_eq!(item["name"], "Apples");
}

#[rstest]
#[tokio::test]
async fn me_is_the_signed_in_person(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (status, mine) = send(&app, req("GET", "/api/me", &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine["sub"], "google-oauth2|me");

    let (status, theirs) = send(&app, req("GET", "/api/me", &them(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(theirs["id"], mine["id"], "two tokens, two people");
    assert_eq!(theirs["sub"], "google-oauth2|someone-else");
}

#[rstest]
#[tokio::test]
async fn me_needs_a_token(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// Read-only on purpose — see the note on the module.
#[rstest]
#[case::edit("PUT")]
#[case::close_the_account("DELETE")]
#[tokio::test]
async fn me_is_not_writable(#[future(awt)] pool: SqlitePool, #[case] method: &str) {
    let app = app(pool);

    let (status, _) = send(&app, req(method, "/api/me", &me(), Some(json!({})))).await;

    assert_eq!(
        status,
        StatusCode::METHOD_NOT_ALLOWED,
        "a write route exists where none was intended"
    );
}

/// A list's history is readable and forgettable by the people on that list.
#[rstest]
#[tokio::test]
async fn history_can_be_read_and_forgotten(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;
    let history = format!("/api/lists/{list_id}/history");

    let (status, mine) = send(&app, req("GET", &history, &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine[0], "Apples", "adding an item did not teach it: {mine}");

    // another person sees none of it
    // someone with no access to the list sees none of its memory, and is told the
    // list does not exist rather than that they may not look
    let (status, _) = send(&app, req("GET", &history, &them(), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        req("DELETE", &format!("{history}/apples"), &them(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        req("DELETE", &format!("{history}/apples"), &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, after) = send(&app, req("GET", &history, &me(), None)).await;
    assert!(after.as_array().unwrap().is_empty(), "{after}");
}
