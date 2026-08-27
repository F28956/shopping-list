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
        // Offering a claim, so that the tests about claiming can drive the real
        // route. Every other test starts from the fixture's already-claimed server,
        // where the code is never looked at.
        ctx: Ctx::new(pool).awaiting_claim("TEST-CODE".into()),
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

/// A third party, for the questions that need somebody who is not either of us.
fn third() -> String {
    "Bearer google-oauth2|third".to_string()
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

/// Reads one SSE frame, or gives up. A stream that stays silent is the failure this
/// route exists to prevent, so waiting forever would turn a bug into a hung suite.
async fn next_event(body: &mut axum::body::BodyDataStream) -> String {
    use futures::StreamExt;
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
        .await
        .expect("no event arrived within 5s")
        .expect("the stream ended instead of sending")
        .expect("the body errored");
    String::from_utf8_lossy(&chunk).into_owned()
}

/// A change made through an ordinary route reaches somebody watching the list --
/// which is the whole point: two devices, one list, no manual refresh.
#[rstest]
#[tokio::test]
async fn a_change_reaches_a_watcher(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;

    let watching = app
        .clone()
        .oneshot(req(
            "GET",
            &format!("/api/lists/{list_id}/events"),
            &me(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(watching.status(), StatusCode::OK);
    assert_eq!(
        watching
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream"),
    );
    let mut body = watching.into_body().into_data_stream();

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

    let frame = next_event(&mut body).await;
    assert!(frame.contains("event: changed"), "got: {frame:?}");
    assert!(frame.contains(&format!("data: {list_id}")), "got: {frame:?}");
}

/// Asserts nothing arrives. The window is short because the events under test are
/// sent in-process, with no network between: anything real turns up immediately.
async fn no_event(body: &mut axum::body::BodyDataStream) {
    use futures::StreamExt;
    let heard = tokio::time::timeout(std::time::Duration::from_millis(500), body.next()).await;
    if let Ok(Some(Ok(chunk))) = heard {
        panic!(
            "heard something it should not have: {:?}",
            String::from_utf8_lossy(&chunk)
        );
    }
}

/// A watcher hears about its own list and no other. Without the filter every device
/// re-reads on every change anyone makes anywhere.
#[rstest]
#[tokio::test]
async fn a_watcher_hears_only_its_own_list(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (watched, _) = a_list_with_an_item(&app).await;
    let (other, other_item) = a_list_with_an_item(&app).await;
    assert_ne!(watched, other);

    let watching = app
        .clone()
        .oneshot(req(
            "GET",
            &format!("/api/lists/{watched}/events"),
            &me(),
            None,
        ))
        .await
        .unwrap();
    let mut body = watching.into_body().into_data_stream();

    // Silence is the assertion: a change to somebody else's list must not wake this
    // stream at all. Reading a frame and checking which list it named would pass
    // whether the filter was there or not, since the id is written from the path.
    send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{other}/items/{other_item}/done"),
            &me(),
            None,
        ),
    )
    .await;
    no_event(&mut body).await;

    // ... and the stream is still live, rather than merely broken.
    send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{watched}/items"),
            &me(),
            Some(json!({"name": "Pears"})),
        ),
    )
    .await;
    let frame = next_event(&mut body).await;
    assert!(frame.contains(&format!("data: {watched}")), "got: {frame:?}");
}

/// Watching is a read, and reads are authorised.
#[rstest]
#[tokio::test]
async fn a_stranger_cannot_watch_my_list(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (status, _) = send(
        &app,
        req("GET", &format!("/api/lists/{list_id}/events"), &them(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a hidden list stays hidden");
}

/// A typed line means the same thing through the API as through the browser.
///
/// This is the bug the phone had: the API took `name` literally, so "2 kg apples"
/// became an item called that, one of it, while the same text in the browser became
/// two kilograms of apples.
#[rstest]
#[tokio::test]
async fn a_typed_line_is_read_the_way_a_person_means_it(
    #[future(awt)]
    #[with(fixtures::UNITS)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (status, list) = send(
        &app,
        req("POST", "/api/lists", &me(), Some(json!({"name": "Fruit"}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let list_id = list["id"].as_i64().unwrap();

    let (status, item) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items"),
            &me(),
            Some(json!({"line": "2 kg apples"})),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(item["name"], "Apples", "the quantity is not part of the name");
    assert_eq!(item["amount"], 2.0);
    assert!(!item["unit_id"].is_null(), "kg was recognised as the unit");
}

/// The structured shape still means exactly what it says. A client that wants an item
/// literally called "1 kg bag of rice" has to be able to have one.
#[rstest]
#[tokio::test]
async fn a_spelled_out_item_is_not_parsed(
    #[future(awt)]
    #[with(fixtures::UNITS)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (_, list) = send(
        &app,
        req("POST", "/api/lists", &me(), Some(json!({"name": "Fruit"}))),
    )
    .await;
    let list_id = list["id"].as_i64().unwrap();

    let (status, item) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items"),
            &me(),
            Some(json!({"name": "1 kg bag of rice"})),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(item["name"], "1 kg bag of rice", "taken literally");
    assert_eq!(item["amount"], 1.0);
    // Not null: something counted rather than measured still has a unit, and `unit`
    // is the one that says so. Left null, this row would never merge with the same
    // thing added as "1 unit ...".
    assert!(
        !item["unit_id"].is_null(),
        "an unmeasured item did not get the `unit` unit"
    );
}

/// Ambiguous or empty is the caller's mistake, not a guess to make on their behalf.
#[rstest]
#[case::both(json!({"line": "2 kg apples", "name": "Apples"}))]
#[case::neither(json!({"amount": 2.0}))]
#[tokio::test]
async fn an_add_must_say_which_it_means(
    #[future(awt)] pool: SqlitePool,
    #[case] body: serde_json::Value,
) {
    let app = app(pool);
    let (_, list) = send(
        &app,
        req("POST", "/api/lists", &me(), Some(json!({"name": "Fruit"}))),
    )
    .await;
    let list_id = list["id"].as_i64().unwrap();

    let (status, _) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/items"),
            &me(),
            Some(body),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

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
    assert_eq!(page["total"], 0, "nothing was added on a refused request");
}

/// The item list carries each row's tags, so a client can group by category the way
/// the browser does without a request per row.
#[rstest]
#[tokio::test]
async fn items_come_with_their_tags(
    #[future(awt)]
    #[with(fixtures::TAGS)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, item_id) = a_list_with_an_item(&app).await;
    let items = format!("/api/lists/{list_id}/items");

    let (_, page) = send(&app, req("GET", &format!("{items}?order_by=id"), &me(), None)).await;
    assert_eq!(
        page["items"][0]["tag_ids"].as_array().unwrap().len(),
        0,
        "an unfiled item has none"
    );

    let (_, tags) = send(&app, req("GET", "/api/tags?order_by=name", &me(), None)).await;
    let tag_id = tags["items"][0]["id"].as_i64().unwrap();
    let (status, _) = send(
        &app,
        req(
            "POST",
            &format!("{items}/{item_id}/tags"),
            &me(),
            Some(json!({"tag_id": tag_id})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, page) = send(&app, req("GET", &format!("{items}?order_by=id"), &me(), None)).await;
    let row = &page["items"][0];
    assert_eq!(row["tag_ids"], json!([tag_id]));
    // Flattened, so the item's own fields are still where they were.
    assert_eq!(row["id"], item_id);
    assert_eq!(row["name"], "Apples");
}

/// A list says what the caller may do with it.
///
/// Without it a client cannot tell a list it owns from one shared for reading, so it
/// either offers controls that will be refused or hides ones the person is entitled
/// to. The browser has always had this; an app could not get it at all.
#[rstest]
#[tokio::test]
async fn a_list_carries_the_callers_role(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (_, page) = send(&app, req("GET", "/api/lists", &me(), None)).await;
    assert_eq!(page["items"][0]["role"], "owner");
    // Flattened, so the list's own fields are where they were.
    assert_eq!(page["items"][0]["id"], list_id);

    let (_, one) = send(&app, req("GET", &format!("/api/lists/{list_id}"), &me(), None)).await;
    assert_eq!(one["role"], "owner", "and on the single-list route too");

    // Somebody invited to read it is told they may read it, and nothing more.
    let (status, invite) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/members/invites"),
            &me(),
            Some(json!({"role": "viewer"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{invite}");
    let token = invite["token"].as_str().expect("no token in {invite}").to_string();

    let (status, _) = send(
        &app,
        req("POST", &format!("/api/invites/{token}"), &them(), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "following the link failed");

    let (_, theirs) = send(&app, req("GET", "/api/lists", &them(), None)).await;
    assert_eq!(
        theirs["items"][0]["role"], "viewer",
        "a guest was told they own it: {theirs}"
    );
    assert_eq!(theirs["items"][0]["id"], list_id);
}

/// A list's tag order: read, set, inherited, and cleared.
#[rstest]
#[tokio::test]
async fn a_lists_tag_order(
    #[future(awt)]
    #[with(fixtures::TAGS)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;
    let path = format!("/api/lists/{list_id}/tag-order");

    // Unconfigured, it is the order a shop is walked.
    let (status, order) = send(&app, req("GET", &path, &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    let positions: Vec<i64> = order
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["sort_order"].as_i64().unwrap())
        .collect();
    assert!(!positions.is_empty());
    assert_eq!(positions, {
        let mut sorted = positions.clone();
        sorted.sort();
        sorted
    });

    // Placing one puts it in front, and leaves the rest where they were.
    let last = order.as_array().unwrap().last().unwrap()["id"].as_i64().unwrap();
    let (status, _) = send(
        &app,
        req("PUT", &path, &me(), Some(json!({"tag_ids": [last]}))),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, order) = send(&app, req("GET", &path, &me(), None)).await;
    assert_eq!(order[0]["id"], last, "what was placed did not lead");
    assert_eq!(
        order.as_array().unwrap().len(),
        positions.len(),
        "a tag went missing"
    );

    // Cleared, it goes back to what it was.
    send(&app, req("PUT", &path, &me(), Some(json!({"tag_ids": []})))).await;
    let (_, order) = send(&app, req("GET", &path, &me(), None)).await;
    assert_ne!(order[0]["id"], last, "clearing did not take");
}

/// The order is one person's view of a shared list, not a change to the list.
#[rstest]
#[tokio::test]
async fn a_tag_order_is_per_person_and_inherited(
    #[future(awt)]
    #[with(fixtures::TAGS)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;
    let path = format!("/api/lists/{list_id}/tag-order");

    let (_, order) = send(&app, req("GET", &path, &me(), None)).await;
    let last = order.as_array().unwrap().last().unwrap()["id"].as_i64().unwrap();
    send(
        &app,
        req("PUT", &path, &me(), Some(json!({"tag_ids": [last]}))),
    )
    .await;

    // Given the list to read, having set nothing, they walk the route I chose.
    let (status, invite) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/members/invites"),
            &me(),
            Some(json!({"role": "viewer"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = invite["token"].as_str().unwrap().to_string();
    send(
        &app,
        req("POST", &format!("/api/invites/{token}"), &them(), None),
    )
    .await;

    let (status, theirs) = send(&app, req("GET", &path, &them(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(theirs[0]["id"], last, "the order was not inherited");

    // A viewer may choose their own, and it does not disturb mine.
    let first = theirs.as_array().unwrap()[1]["id"].as_i64().unwrap();
    let (status, _) = send(
        &app,
        req("PUT", &path, &them(), Some(json!({"tag_ids": [first]}))),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "a viewer could not order");

    let (_, mine) = send(&app, req("GET", &path, &me(), None)).await;
    assert_eq!(mine[0]["id"], last, "their choice changed mine");
}

/// A stranger cannot read a list's order, nor set one.
#[rstest]
#[tokio::test]
async fn a_stranger_cannot_touch_a_tag_order(
    #[future(awt)]
    #[with(fixtures::TAGS)]
    pool: SqlitePool,
) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;
    let path = format!("/api/lists/{list_id}/tag-order");

    let (status, _) = send(&app, req("GET", &path, &them(), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app,
        req("PUT", &path, &them(), Some(json!({"tag_ids": [1]}))),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Sharing a list, end to end: invite, join, see who is on it, and leave.
#[rstest]
#[tokio::test]
async fn sharing_a_list(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;
    let members = format!("/api/lists/{list_id}/members");

    // On my own list, I am on it and I own it.
    let (status, people) = send(&app, req("GET", &members, &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(people.as_array().unwrap().len(), 1);
    assert_eq!(people[0]["role"], "owner");
    // The keys are carried even when this harness has nobody to put in them: it
    // signs people in from a bearer subject alone, so they have no name or address.
    // That they are filled from the user is checked where users have one -- see the
    // service's `people_on` tests.
    assert!(people[0].get("name").is_some(), "no name field at all: {people}");
    assert!(people[0].get("email").is_some(), "no email field at all: {people}");

    let (status, invite) = send(
        &app,
        req(
            "POST",
            &format!("{members}/invites"),
            &me(),
            Some(json!({"role": "editor"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = invite["token"].as_str().expect("no token").to_string();

    // Before joining, the list is not theirs to see.
    let (status, theirs) = send(&app, req("GET", "/api/lists", &them(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(theirs["total"], 0, "a list turned up unshared: {theirs}");

    let (status, joined) = send(
        &app,
        req("POST", &format!("/api/invites/{token}"), &them(), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(joined["id"], list_id);

    // Now both of us are on it, and each is named.
    let (_, people) = send(&app, req("GET", &members, &me(), None)).await;
    assert_eq!(people.as_array().unwrap().len(), 2);
    let roles: Vec<&str> = people
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, vec!["owner", "editor"]);

    // They can see it too, and see who they are sharing with.
    let (status, theirs) = send(&app, req("GET", &members, &them(), None)).await;
    assert_eq!(status, StatusCode::OK, "a member cannot see the members");
    assert_eq!(theirs.as_array().unwrap().len(), 2);

    // Leaving is removing yourself.
    let mine = people[0]["user_id"].as_i64().unwrap();
    let theirs_id = people[1]["user_id"].as_i64().unwrap();
    assert_ne!(mine, theirs_id);

    let (status, _) = send(
        &app,
        req("DELETE", &format!("{members}/{theirs_id}"), &them(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, gone) = send(&app, req("GET", "/api/lists", &them(), None)).await;
    assert_eq!(gone["total"], 0, "leaving did not take");
}

/// A link admits the person it was written for, and nobody after them.
///
/// Following it twice is a double-click and stays harmless; a third party arriving
/// with the same link a day later does not get in. A withdrawn link never works.
#[rstest]
#[tokio::test]
async fn a_used_invitation_admits_nobody_else(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;
    let invites = format!("/api/lists/{list_id}/members/invites");

    let (_, invite) = send(
        &app,
        req("POST", &invites, &me(), Some(json!({"role": "editor"}))),
    )
    .await;
    let token = invite["token"].as_str().unwrap().to_string();
    let follow = format!("/api/invites/{token}");

    let (status, _) = send(&app, req("POST", &follow, &them(), None)).await;
    assert_eq!(status, StatusCode::OK);

    // The same person again: a double-click, and harmless.
    let (status, _) = send(&app, req("POST", &follow, &them(), None)).await;
    assert_eq!(status, StatusCode::OK, "a double-click was refused");

    // Somebody else with the same link: not admitted.
    let (status, _) = send(&app, req("POST", &follow, &third(), None)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a spent link admitted a second person"
    );

    // And a fresh one, withdrawn before it is followed, works for nobody.
    let (_, invite) = send(
        &app,
        req("POST", &invites, &me(), Some(json!({"role": "editor"}))),
    )
    .await;
    let token = invite["token"].as_str().unwrap().to_string();
    send(&app, req("DELETE", &invites, &me(), None)).await;

    let (status, _) = send(
        &app,
        req("POST", &format!("/api/invites/{token}"), &third(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a withdrawn link still worked");
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
    // Qualified by the provider that vouched for it. A subject is only unique within
    // the provider that issued it, and `users.sub` is unique across the table -- so an
    // account created since there was more than one provider records both halves. Who
    // this person is now lives in `user_identities`; this is a record of how they
    // arrived.
    assert_eq!(mine["sub"], "google|google-oauth2|me");

    let (status, theirs) = send(&app, req("GET", "/api/me", &them(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(theirs["id"], mine["id"], "two tokens, two people");
    assert_eq!(theirs["sub"], "google|google-oauth2|someone-else");
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

/// The profile is still read-only — see the note on the module. Closing the account
/// is not, and has its own tests further down.
#[rstest]
#[tokio::test]
async fn the_profile_is_not_writable(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (status, _) = send(&app, req("PUT", "/api/me", &me(), Some(json!({})))).await;

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

/// The wire shape of `POST /api/sync`, end to end.
///
/// The service tests cover what each operation means; this covers that the route can be
/// spoken to — the tagged JSON, the RFC 3339 stamp, the uuids in place of ids, and the
/// answer coming back per operation rather than as one status code.
#[rstest]
#[tokio::test]
async fn a_batch_replays_over_the_wire(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (_, list) = send(&app, req("GET", &format!("/api/lists/{list_id}"), &me(), None)).await;
    let list_uuid = list["uuid"].as_str().unwrap().to_string();

    let (_, page) = send(
        &app,
        req("GET", &format!("/api/lists/{list_id}/items"), &me(), None),
    )
    .await;
    let apples = page["items"][0]["uuid"].as_str().unwrap().to_string();

    let mine = "11111111-1111-4111-8111-111111111111";
    let batch = json!({
        "operations": [
            {
                "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "at": "2026-08-26T10:00:00Z",
                "list": list_uuid,
                "kind": "set_done",
                "item": apples,
                "done": true
            },
            {
                "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "at": "2026-08-26T10:01:00Z",
                "list": list_uuid,
                "kind": "add",
                "item": mine,
                "name": "Bread"
            }
        ]
    });

    let (status, replayed) = send(&app, req("POST", "/api/sync", &me(), Some(batch.clone()))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed["operations"][0]["outcome"], "applied");
    assert_eq!(replayed["operations"][1]["outcome"], "applied");
    // The row the device made is handed back, so it can learn the id it never had.
    assert_eq!(replayed["operations"][1]["item"]["uuid"], mine);
    assert!(replayed["operations"][1]["item"]["id"].as_i64().unwrap() > 0);
    // Stamped with what the device claimed, not with now.
    assert_eq!(
        replayed["operations"][0]["item"]["done_at"],
        "2026-08-26T10:00:00Z"
    );

    // And again, which is what a lost answer produces.
    let (status, again) = send(&app, req("POST", "/api/sync", &me(), Some(batch))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["operations"][0]["outcome"], "already_applied");
    assert_eq!(again["operations"][1]["outcome"], "already_applied");

    let (_, page) = send(
        &app,
        req("GET", &format!("/api/lists/{list_id}/items"), &me(), None),
    )
    .await;
    assert_eq!(page["total"], 2, "the resend added a row");
}

/// A refusal is data, not a status code.
///
/// Somebody who is not on the list gets `200` with every operation refused, because the
/// request was fine — it is the changes in it that were not.
#[rstest]
#[tokio::test]
async fn a_refused_batch_is_still_a_two_hundred(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (_, list) = send(&app, req("GET", &format!("/api/lists/{list_id}"), &me(), None)).await;
    let list_uuid = list["uuid"].as_str().unwrap().to_string();

    let (status, replayed) = send(
        &app,
        req(
            "POST",
            "/api/sync",
            &them(),
            Some(json!({
                "operations": [{
                    "id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                    "at": "2026-08-26T10:00:00Z",
                    "list": list_uuid,
                    "kind": "add",
                    "item": "22222222-2222-4222-8222-222222222222",
                    "name": "Not theirs"
                }]
            })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed["operations"][0]["outcome"], "refused");
    assert_eq!(replayed["operations"][0]["why"], "not_allowed");
}

/// The whole point of the exchange: what comes back works everywhere the provider's
/// token did.
///
/// Apple's identity token lasts about ten minutes, so an Apple client trades it once
/// for this and never shows the provider's token again.
#[rstest]
#[tokio::test]
async fn a_session_token_stands_in_for_the_one_that_bought_it(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (status, issued) = send(&app, req("POST", "/api/sessions", &me(), None)).await;
    assert_eq!(status, StatusCode::OK, "{issued}");

    let token = issued["token"].as_str().expect("no token").to_string();
    assert_eq!(token.len(), 64, "not an opaque token: {token}");
    assert!(issued["idle_seconds"].as_i64().unwrap() > 0);

    let (status, whoami) = send(
        &app,
        req("GET", "/api/me", &format!("Bearer {token}"), None),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{whoami}");
    assert_eq!(whoami["sub"], "google|google-oauth2|me");
}

/// Signing out has to end it, or the session outlives the person's decision.
#[rstest]
#[tokio::test]
async fn ending_a_session_stops_the_token_working(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (_, issued) = send(&app, req("POST", "/api/sessions", &me(), None)).await;
    let auth = format!("Bearer {}", issued["token"].as_str().unwrap());

    let (status, _) = send(&app, req("DELETE", "/api/sessions", &auth, None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&app, req("GET", "/api/me", &auth, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Two devices, two sessions. Signing out on the phone must not sign out the Mac.
#[rstest]
#[tokio::test]
async fn one_device_signing_out_leaves_the_others_alone(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (_, phone) = send(&app, req("POST", "/api/sessions", &me(), None)).await;
    let (_, mac) = send(&app, req("POST", "/api/sessions", &me(), None)).await;

    let phone = format!("Bearer {}", phone["token"].as_str().unwrap());
    let mac = format!("Bearer {}", mac["token"].as_str().unwrap());
    assert_ne!(phone, mac, "two sign-ins produced the same token");

    send(&app, req("DELETE", "/api/sessions", &phone, None)).await;

    let (status, _) = send(&app, req("GET", "/api/me", &mac, None)).await;
    assert_eq!(status, StatusCode::OK, "the other device was signed out too");
}

/// An invented token of the right shape is still nobody.
///
/// The shape test is how a session token is told apart from a JWT, so it must not also
/// be how one is accepted.
#[rstest]
#[tokio::test]
async fn a_token_shaped_like_a_session_but_issued_by_nobody(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let invented = "f".repeat(64);

    let (status, _) = send(
        &app,
        req("GET", "/api/me", &format!("Bearer {invented}"), None),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// One person's session must not reach another person's list.
#[rstest]
#[tokio::test]
async fn a_session_carries_one_identity_and_not_another(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (_, issued) = send(&app, req("POST", "/api/sessions", &them(), None)).await;
    let auth = format!("Bearer {}", issued["token"].as_str().unwrap());

    let (status, _) = send(&app, req("GET", &format!("/api/lists/{list_id}"), &auth, None)).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Who may use this server
// ---------------------------------------------------------------------------

/// A server with an owner and a guest, both admitted, reached over the wire.
///
/// Claimed through the route rather than by writing rows, so the test exercises the
/// path a real first boot takes.
async fn a_claimed_server(pool: &SqlitePool) -> Router {
    let app = app(pool.clone());

    sqlx::raw_sql("UPDATE server SET admits_anyone = 0, claimed_at = NULL")
        .execute(pool)
        .await
        .unwrap();

    let (status, body) = send(
        &app,
        req("POST", "/api/server/claim", &me(), Some(json!({"code": "TEST-CODE"}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    app
}

#[rstest]
#[tokio::test]
async fn an_unclaimed_server_says_so_before_anybody_signs_in(#[future(awt)] pool: SqlitePool) {
    let app = app(pool.clone());
    sqlx::raw_sql("UPDATE server SET claimed_at = NULL")
        .execute(&pool)
        .await
        .unwrap();

    // No authorization header at all: a sign-in screen has to be able to ask this
    // before there is anybody to ask as.
    let unauthenticated = Request::builder()
        .uri("/api/server")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let (status, body) = send(&app, unauthenticated).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["admission"], "unclaimed");
    // C2: a client accepts an address when the name matches, so that pointing the app
    // at an unrelated service fails here rather than at the first API call.
    assert_eq!(body["name"], "shopping-list");
    assert!(body["version"].as_str().is_some_and(|v| !v.is_empty()));
}

#[rstest]
#[tokio::test]
async fn claiming_signs_the_owner_in(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    let (status, body) = send(&app, req("GET", "/api/me", &me(), None)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["is_owner"], true);
    assert_eq!(body["sub"], "google|google-oauth2|me");
}

/// The land grab A2 is about, over the wire.
#[rstest]
#[tokio::test]
async fn the_wrong_code_claims_nothing_over_the_wire(#[future(awt)] pool: SqlitePool) {
    let app = app(pool.clone());
    sqlx::raw_sql("UPDATE server SET admits_anyone = 0, claimed_at = NULL")
        .execute(&pool)
        .await
        .unwrap();

    let (status, _) = send(
        &app,
        req("POST", "/api/server/claim", &them(), Some(json!({"code": "NOPE-NOPE"}))),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, body) = send(&app, req("GET", "/api/server", &me(), None)).await;
    assert_eq!(body["admission"], "unclaimed", "a wrong code claimed the server anyway");
}

#[rstest]
#[tokio::test]
async fn an_owner_manages_who_may_sign_in(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    let (status, _) = send(
        &app,
        req("POST", "/api/admissions", &me(), Some(json!({"email": "her@example.com", "note": "mum"}))),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, listed) = send(&app, req("GET", "/api/admissions", &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    let rows = listed.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{listed}");
    assert!(rows.iter().any(|r| r["email"] == "her@example.com" && r["note"] == "mum"));

    let (status, _) = send(
        &app,
        req("DELETE", "/api/admissions/her@example.com", &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, listed) = send(&app, req("GET", "/api/admissions", &me(), None)).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

/// Every route on the owner's screen, refused to somebody who merely uses the server.
#[rstest]
#[tokio::test]
async fn a_guest_is_refused_every_owner_route(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    // Admitted, so this is about administering and not about signing in.
    send(
        &app,
        req("POST", "/api/admissions", &me(), Some(json!({"email": "google-oauth2|someone-else@example.com"}))),
    )
    .await;

    for (method, path, body) in [
        ("GET", "/api/admissions", None),
        ("POST", "/api/admissions", Some(json!({"email": "x@example.com"}))),
        ("DELETE", "/api/admissions/x@example.com", None),
        ("POST", "/api/admissions/x@example.com/owner", None),
        ("DELETE", "/api/admissions/x@example.com/owner", None),
        ("PUT", "/api/server", Some(json!({"admits_anyone": true}))),
    ] {
        let (status, _) = send(&app, req(method, path, &them(), body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} was allowed");
    }
}

/// A5 over the wire: the way to lock everybody out is refused at the door.
#[rstest]
#[tokio::test]
async fn the_last_owner_cannot_demote_themselves_over_the_wire(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    let (_, listed) = send(&app, req("GET", "/api/admissions", &me(), None)).await;
    let mine = listed[0]["email"].as_str().unwrap().to_string();

    let (status, _) = send(
        &app,
        req("DELETE", &format!("/api/admissions/{mine}/owner"), &me(), None),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = send(&app, req("DELETE", &format!("/api/admissions/{mine}"), &me(), None)).await;
    assert_eq!(status, StatusCode::CONFLICT, "the last owner withdrew themselves");
}

/// A6: open is a setting somebody turned on, and it works.
#[rstest]
#[tokio::test]
async fn an_owner_can_open_the_server(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    // Closed: a stranger cannot get in.
    let (status, _) = send(&app, req("GET", "/api/me", &third(), None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app,
        req("PUT", "/api/server", &me(), Some(json!({"admits_anyone": true}))),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&app, req("GET", "/api/me", &third(), None)).await;
    assert_eq!(status, StatusCode::OK, "the server was opened and still refused somebody");

    // And it says so, so a sign-in screen can stop promising a refusal that will not
    // come.
    let (_, about) = send(&app, req("GET", "/api/server", &me(), None)).await;
    assert_eq!(about["admission"], "open");
}

// ---------------------------------------------------------------------------
// Closing an account
// ---------------------------------------------------------------------------

/// The ordinary case: everything of theirs goes, and the token stops working.
#[rstest]
#[tokio::test]
async fn closing_an_account_takes_it_all(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    let (status, _) = send(&app, req("DELETE", "/api/me", &me(), None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Signing in again is a new person, who has nothing.
    let (status, listed) = send(&app, req("GET", "/api/lists", &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["items"].as_array().unwrap().len(), 0, "{listed}");

    let (status, _) = send(&app, req("GET", &format!("/api/lists/{list_id}"), &me(), None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A shared list changes hands rather than vanishing.
///
/// The person left behind did nothing, and cascading would have them open the app to
/// find their shopping gone.
#[rstest]
#[tokio::test]
async fn a_shared_list_survives_its_owner_leaving(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let (list_id, _) = a_list_with_an_item(&app).await;

    // Share it, the way sharing actually happens.
    let (status, invite) = send(
        &app,
        req(
            "POST",
            &format!("/api/lists/{list_id}/members/invites"),
            &me(),
            Some(json!({"role": "editor"})),
        ),
    )
    .await;
    assert!(status.is_success(), "{status} {invite}");
    let token = invite["token"].as_str().expect("no token").to_string();

    let (status, joined) = send(&app, req("POST", &format!("/api/invites/{token}"), &them(), None)).await;
    assert!(status.is_success(), "{status} {joined}");

    send(&app, req("DELETE", "/api/me", &me(), None)).await;

    let (status, list) = send(&app, req("GET", &format!("/api/lists/{list_id}"), &them(), None)).await;
    assert_eq!(status, StatusCode::OK, "the list went with its owner: {list}");
    assert_eq!(list["role"], "owner", "it survived but nobody owns it");
}

/// A5 from a third direction. Closing the last owner's account would leave a server
/// nobody can administer, and the way back involves a shell on the host.
#[rstest]
#[tokio::test]
async fn the_last_owner_cannot_close_their_account(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    let (status, _) = send(&app, req("DELETE", "/api/me", &me(), None)).await;

    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = send(&app, req("GET", "/api/me", &me(), None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Closing an account forgets the address too, or a person who asked to be erased is
/// still listed on somebody's admission screen.
#[rstest]
#[tokio::test]
async fn closing_an_account_forgets_the_address(#[future(awt)] pool: SqlitePool) {
    let app = a_claimed_server(&pool).await;

    let guest = "google-oauth2|someone-else@example.com";
    send(
        &app,
        req("POST", "/api/admissions", &me(), Some(json!({"email": guest}))),
    )
    .await;
    // Signing in binds the address to them.
    send(&app, req("GET", "/api/me", &them(), None)).await;

    send(&app, req("DELETE", "/api/me", &them(), None)).await;

    let (_, listed) = send(&app, req("GET", "/api/admissions", &me(), None)).await;
    assert!(
        !listed.as_array().unwrap().iter().any(|r| r["email"] == guest),
        "the address outlived the account: {listed}"
    );
}

// ---------------------------------------------------------------------------
// A list made with no signal at all
// ---------------------------------------------------------------------------

/// S1. The whole point: a list and its first item can be made offline, in one batch,
/// and arrive as a list with an item on it.
///
/// Nothing else in the sync protocol had to change for this, because every operation
/// already names its list by `uuid` rather than by an id the device could not have.
#[rstest]
#[tokio::test]
async fn a_list_made_offline_arrives_with_its_items(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    let (status, replies) = send(
        &app,
        req("POST", "/api/sync", &me(), Some(json!({"operations": [
            {"id": "aaaaaaaa-0001-4000-8000-000000000001", "at": "2026-08-27T10:00:00Z", "list": "11111111-1111-4111-8111-111111111111",
             "kind": "make_list", "name": "Camping"},
            {"id": "aaaaaaaa-0002-4000-8000-000000000002", "at": "2026-08-27T10:00:01Z", "list": "11111111-1111-4111-8111-111111111111",
             // Named rather than typed as a line: what this test is about is the
             // list arriving with its items, and the fixture seeds no units, so a
             // typed "2 kg potatoes" would be about unit parsing instead.
             "kind": "add", "item": "22222222-2222-4222-8222-222222222222",
             "name": "potatoes", "amount": 2},
        ]}))),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{replies}");
    let outcomes = replies["operations"].as_array().unwrap();
    assert_eq!(outcomes[0]["outcome"], "applied", "{replies}");
    assert_eq!(outcomes[0]["list"]["name"], "Camping");
    assert_eq!(outcomes[1]["outcome"], "applied", "{replies}");

    // The id the device could not have known, handed back so the next batch can use it.
    let id = outcomes[0]["list"]["id"].as_i64().unwrap();
    let (status, items) = send(&app, req("GET", &format!("/api/lists/{id}/items"), &me(), None)).await;
    assert_eq!(status, StatusCode::OK);
    // Capitalised on the way in, as every other route does.
    assert_eq!(items["items"][0]["name"], "Potatoes");
    assert_eq!(items["items"][0]["amount"], 2.0);
}

/// A queue never expires and a reply can be lost, so the same batch arriving twice is
/// ordinary. It must not make two lists.
#[rstest]
#[tokio::test]
async fn making_the_same_list_twice_makes_one(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);
    let batch = json!({"operations": [
        {"id": "aaaaaaaa-0001-4000-8000-000000000001", "at": "2026-08-27T10:00:00Z", "list": "11111111-1111-4111-8111-111111111111",
         "kind": "make_list", "name": "Camping"},
    ]});

    let (_, first) = send(&app, req("POST", "/api/sync", &me(), Some(batch.clone()))).await;
    let (_, again) = send(&app, req("POST", "/api/sync", &me(), Some(batch))).await;

    assert_eq!(first["operations"][0]["outcome"], "applied");
    assert_eq!(again["operations"][0]["outcome"], "already_applied");
    // Answered with the list either way, because the device still does not know what
    // the server calls it — a lost reply is exactly why it is resending.
    assert_eq!(
        again["operations"][0]["list"]["id"],
        first["operations"][0]["list"]["id"]
    );

    let (_, listed) = send(&app, req("GET", "/api/lists", &me(), None)).await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1, "{listed}");
}

/// Guessing a uuid is not a way into anybody's shopping: the list is found, it is not
/// theirs, and they are told it is gone.
#[rstest]
#[tokio::test]
async fn making_a_list_over_somebody_elses_uuid(#[future(awt)] pool: SqlitePool) {
    let app = app(pool);

    send(
        &app,
        req("POST", "/api/sync", &me(), Some(json!({"operations": [
            {"id": "aaaaaaaa-0001-4000-8000-000000000001", "at": "2026-08-27T10:00:00Z", "list": "11111111-1111-4111-8111-111111111111",
             "kind": "make_list", "name": "Mine"},
        ]}))),
    )
    .await;

    // Somebody else claims the same uuid, then tries to write to it.
    let (_, theirs) = send(
        &app,
        req("POST", "/api/sync", &them(), Some(json!({"operations": [
            {"id": "aaaaaaaa-0002-4000-8000-000000000002", "at": "2026-08-27T10:00:00Z", "list": "11111111-1111-4111-8111-111111111111",
             "kind": "make_list", "name": "Theirs"},
            {"id": "aaaaaaaa-0003-4000-8000-000000000003", "at": "2026-08-27T10:00:01Z", "list": "11111111-1111-4111-8111-111111111111",
             "kind": "add", "item": "33333333-3333-4333-8333-333333333333", "line": "spying"},
        ]}))),
    )
    .await;

    // The list is found and is not theirs, so the write that follows is refused.
    assert_eq!(theirs["operations"][1]["outcome"], "refused", "{theirs}");

    let (_, mine) = send(&app, req("GET", "/api/lists", &me(), None)).await;
    assert_eq!(mine["items"][0]["name"], "Mine", "the name was taken over");
    let id = mine["items"][0]["id"].as_i64().unwrap();
    let (_, items) = send(&app, req("GET", &format!("/api/lists/{id}/items"), &me(), None)).await;
    assert_eq!(items["items"].as_array().unwrap().len(), 0, "somebody wrote to my list");
}
