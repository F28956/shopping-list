//! The pages, driven through the real router with a real session.
//!
//! These are what verify the half of the application a person actually touches: that
//! a list renders, that ticking an item off changes what comes back, and that one
//! person's list is not on another person's screen.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use domain::models::user::{Name, Sub, User};
use domain::models::{fixtures, pool};
use domain::service::Ctx;
use http_body_util::BodyExt;
use rstest::rstest;
use sqlx::SqlitePool;
use tower::ServiceExt;

use crate::{auth, router, session_store, testing::offline_state};

/// A router plus a signed-in session cookie for `sub`.
///
/// The session is written straight into the store and the cookie built from its id,
/// rather than driving a request first: the session layer only sets a cookie when
/// something modified the session, and a signed-out page view does not.
async fn signed_in(pool: &SqlitePool, sub: &str) -> (axum::Router, String) {
    let ctx = Ctx::new(pool.clone());
    let store = session_store(&ctx).await.unwrap();
    let app = router(offline_state(ctx), store.clone());

    let user = User::find_or_create(pool, Sub(sub.into()), Some(Name(sub.into())), None)
        .await
        .unwrap();

    let id = tower_sessions::session::Id::default();
    let mut record = tower_sessions::session::Record {
        id,
        data: Default::default(),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::days(1),
    };
    record
        .data
        .insert(auth::USER_ID.to_string(), serde_json::json!(user.id.0));
    tower_sessions::SessionStore::save(&store, &record)
        .await
        .unwrap();

    // "id" is tower-sessions' default cookie name.
    (app, format!("id={id}"))
}

async fn get(app: &axum::Router, uri: &str, cookie: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
        .unwrap_or_default();
    (status, body)
}

/// Like `post`, but announcing itself as htmx — which is what makes the handler
/// answer with a fragment instead of a redirect.
async fn post_htmx(
    app: &axum::Router,
    uri: &str,
    cookie: &str,
    form: &str,
) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .method("POST")
                .header(header::COOKIE, cookie)
                .header("hx-request", "true")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
        .unwrap_or_default();
    (status, body)
}

/// Like `post`, but for the handlers that answer with a page rather than a redirect.
async fn post_page(app: &axum::Router, uri: &str, cookie: &str, form: &str) -> String {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .method("POST")
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
        .unwrap_or_default()
}

async fn post(app: &axum::Router, uri: &str, cookie: &str, form: &str) -> StatusCode {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .method("POST")
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    res.status()
}

/// Whether any item's edit panel came back expanded.
///
/// Checks the switch's own tag rather than the page for the word "checked", which
/// would match anything else on it.
fn panel_is_open(html: &str) -> bool {
    html.match_indices("class=\"panel-switch\"").any(|(at, _)| {
        let tag_end = html[at..].find('>').map(|e| at + e).unwrap_or(html.len());
        html[at..tag_end].contains("checked")
    })
}

/// The first tag the picker actually offers, skipping the placeholder option.
fn first_tag_option(html: &str) -> Option<i64> {
    let select = html.find("name=\"tag_id\"")?;
    html[select..]
        .match_indices("value=\"")
        .map(|(at, _)| {
            html[select + at + 7..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .find(|v| !v.is_empty())
        .and_then(|v| v.parse().ok())
}

/// Pulls the id out of the first `/lists/{id}` link on the page.
fn first_list_id(html: &str) -> i64 {
    let at = html.find("/lists/").expect("no list link on the page");
    html[at + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("list link had no id")
}

#[rstest]
#[tokio::test]
async fn a_signed_out_visitor_is_offered_a_sign_in(#[future(awt)] pool: SqlitePool) {
    let ctx = Ctx::new(pool.clone());
    let store = session_store(&ctx).await.unwrap();
    let app = router(offline_state(ctx), store);

    let (status, body) = get(&app, "/", "").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sign in with Google"), "{body}");
    assert!(
        !body.contains("Add list"),
        "the form is for people who are signed in"
    );
}

#[rstest]
#[tokio::test]
async fn lists_and_items_render_and_change(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|shopper").await;

    // empty state
    let (status, body) = get(&app, "/", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No lists yet"), "{body}");

    // add a list
    assert_eq!(
        post(&app, "/lists", &cookie, "name=Fruit+%26+veg").await,
        StatusCode::SEE_OTHER
    );
    let (_, body) = get(&app, "/", &cookie).await;
    assert!(
        body.contains("Fruit &amp; veg"),
        "the name is escaped, not raw: {body}"
    );
    let list_id = first_list_id(&body);

    // the list page, empty
    let (status, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Nothing on this list yet"), "{body}");
    assert!(
        body.contains("name=\"line\""),
        "the quick-add field is missing: {body}"
    );
    assert!(
        body.contains("list=\"item-history\""),
        "the quick-add field is not backed by history: {body}"
    );

    // add an item with a unit
    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/items"),
            &cookie,
            "line=2+Apples"
        )
        .await,
        StatusCode::SEE_OTHER
    );
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(body.contains("Apples"), "{body}");
    assert!(body.contains("☐"), "an outstanding item shows an empty box");

    // tick it off
    let item_at = body.find("/items/").unwrap();
    let item_id: i64 = body[item_at + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/items/{item_id}/toggle"),
            &cookie,
            ""
        )
        .await,
        StatusCode::SEE_OTHER
    );
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(body.contains("☑"), "ticking it off did not show: {body}");
    assert!(
        body.contains("item done"),
        "the row is not marked done: {body}"
    );

    // and back again
    post(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/toggle"),
        &cookie,
        "",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(body.contains("☐"), "unticking did not show");
}

/// The service layer's rule, seen from the browser: another person's list is a 404
/// page, not a hint that it exists.
#[rstest]
#[tokio::test]
async fn one_persons_list_is_not_on_anothers_screen(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Private").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let (app2, theirs) = signed_in(&pool, "google-oauth2|stranger").await;

    let (status, body) = get(&app2, "/", &theirs).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("No lists yet"),
        "someone else's list appeared: {body}"
    );

    let (status, _) = get(&app2, &format!("/lists/{list_id}"), &theirs).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_eq!(
        post(
            &app2,
            &format!("/lists/{list_id}/items"),
            &theirs,
            "line=smuggled"
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        post(&app2, &format!("/lists/{list_id}/delete"), &theirs, "").await,
        StatusCode::NOT_FOUND
    );

    // the owner's list is untouched
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &mine).await;
    assert!(body.contains("Nothing on this list yet"), "{body}");
}

#[rstest]
#[tokio::test]
async fn notes_still_work(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|note-taker").await;

    assert_eq!(
        post(&app, "/notes", &cookie, "body=bring+the+bags").await,
        StatusCode::SEE_OTHER
    );

    let (status, body) = get(&app, "/notes", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("bring the bags"), "{body}");
}

/// An empty field is a 400, not a 500 — the CHECK constraint reaches the browser as
/// the caller's mistake.
#[rstest]
#[tokio::test]
async fn an_empty_name_is_a_client_error(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|fumble").await;

    assert_eq!(
        post(&app, "/lists", &cookie, "name=+++").await,
        StatusCode::BAD_REQUEST
    );
}

/// Signed out, every page that acts sends you to the login rather than erroring.
#[rstest]
#[case::a_list("/lists/1")]
#[case::notes("/notes")]
#[tokio::test]
async fn acting_while_signed_out_redirects_to_login(
    #[future(awt)] pool: SqlitePool,
    #[case] uri: &str,
) {
    let ctx = Ctx::new(pool);
    let store = session_store(&ctx).await.unwrap();
    let app = router(offline_state(ctx), store);

    let (status, _) = get(&app, uri, "").await;

    assert_eq!(status, StatusCode::SEE_OTHER);
}

/// Tagging, from the browser: the chip appears, the picker stops offering what is
/// already on the item, and removing it takes the chip away.
#[rstest]
#[tokio::test]
async fn tagging_an_item_from_the_page(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|tagger").await;
    post(&app, "/lists", &cookie, "name=Bakery").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Bagels",
    )
    .await;

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    assert!(body.contains("+ tag"), "no tag picker on the page: {body}");

    // pick the first option that carries an id; the select leads with a disabled
    // "+ tag" placeholder whose value is empty
    let tag_id = first_tag_option(&body).expect("no tag options -- are tags seeded?");

    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/items/{item_id}/tags"),
            &cookie,
            &format!("tag_id={tag_id}")
        )
        .await,
        StatusCode::SEE_OTHER
    );

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    // The first tag names the group the item now sits under rather than repeating as
    // a chip beside it; the removable chip in the edit panel is the one to look for.
    assert!(
        body.contains("class=\"chip removable\""),
        "the tag did not attach: {body}"
    );
    assert!(
        body.contains("class=\"group-heading\""),
        "the item is not grouped under its tag: {body}"
    );
    let offered = body.matches(&format!("value=\"{tag_id}\"")).count();
    assert_eq!(
        offered, 0,
        "the picker still offers a tag already on the item"
    );

    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/items/{item_id}/tags/{tag_id}/delete"),
            &cookie,
            ""
        )
        .await,
        StatusCode::SEE_OTHER
    );
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(
        !body.contains("class=\"chip\""),
        "the chip survived removal"
    );
}

/// Another person's item cannot be tagged, even knowing both ids.
#[rstest]
#[tokio::test]
async fn a_stranger_cannot_tag_my_item_from_the_page(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Private").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &mine,
        "line=Secret",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &mine).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|stranger").await;

    assert_eq!(
        post(
            &app2,
            &format!("/lists/{list_id}/items/{item_id}/tags"),
            &theirs,
            "tag_id=1"
        )
        .await,
        StatusCode::NOT_FOUND
    );
}

/// A change made anywhere reaches a browser left open on the list.
///
/// Driven through the real routes rather than the notifier directly: the thing worth
/// proving is that the browser's own stream is wired to the same changes the API
/// announces, since the two transports build their state separately.
#[rstest]
#[tokio::test]
async fn a_change_reaches_a_browser_watcher(#[future(awt)] pool: SqlitePool) {
    use futures::StreamExt;

    let (app, cookie) = signed_in(&pool, "google-oauth2|watcher").await;
    post(&app, "/lists", &cookie, "name=Groceries").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);

    // Not `get`, which collects the whole body: an event stream has no end.
    let watching = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/lists/{list_id}/events"))
                .header(header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(watching.status(), StatusCode::OK);
    let mut stream = watching.into_body().into_data_stream();

    post_htmx(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=2+kg+apples",
    )
    .await;

    let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("no event arrived within 5s")
        .expect("the stream ended instead of sending")
        .expect("the body errored");
    let frame = String::from_utf8_lossy(&chunk).into_owned();
    assert!(frame.contains("event: changed"), "got: {frame:?}");
    assert!(
        frame.contains(&format!("data: {list_id}")),
        "got: {frame:?}"
    );
}

/// Filing an item is a change to the list like any other.
///
/// Worth its own test because tags are attached by a different service than the one
/// that edits an item, and a notifier wired into only some of the mutations looks
/// exactly like a working one until you use the mutation it missed.
#[rstest]
#[tokio::test]
async fn a_tag_change_reaches_a_watcher(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    use futures::StreamExt;

    let (app, cookie) = signed_in(&pool, "google-oauth2|filer").await;
    post(&app, "/lists", &cookie, "name=Groceries").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Apples",
    )
    .await;

    let (_, page) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = page[page.find("/items/").expect("no item on the page") + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("item link had no id");
    let tag_id = first_tag_option(&page).expect("no tag options");

    let watching = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/lists/{list_id}/events"))
                .header(header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut stream = watching.into_body().into_data_stream();

    post(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/tags"),
        &cookie,
        &format!("tag_id={tag_id}"),
    )
    .await;

    let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("filing an item told nobody")
        .expect("the stream ended instead of sending")
        .expect("the body errored");
    assert!(
        String::from_utf8_lossy(&chunk).contains(&format!("data: {list_id}")),
        "wrong list"
    );
}

/// The tag order page reorders the list it belongs to.
#[rstest]
#[tokio::test]
async fn a_tag_can_be_moved_up_the_order(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|sorter").await;
    post(&app, "/lists", &cookie, "name=Shop").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);

    let (status, page) = get(&app, &format!("/lists/{list_id}/tags"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Tag order"), "{page}");

    // The last tag in the order, moved to the front one step at a time, leads.
    let last = page
        .rmatch_indices("name=\"tag_id\" value=\"")
        .next()
        .map(|(at, m)| {
            page[at + m.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .expect("no tags on the page");

    for _ in 0..40 {
        post(
            &app,
            &format!("/lists/{list_id}/tags/move"),
            &cookie,
            &format!("tag_id={last}&up=true"),
        )
        .await;
    }

    let (_, page) = get(&app, &format!("/lists/{list_id}/tags"), &cookie).await;
    let first = page[page.find("name=\"tag_id\" value=\"").unwrap() + 21..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    assert_eq!(first, last, "it did not reach the front");

    // Reset puts the shop's own order back.
    post(&app, &format!("/lists/{list_id}/tags/reset"), &cookie, "").await;
    let (_, page) = get(&app, &format!("/lists/{list_id}/tags"), &cookie).await;
    let first = page[page.find("name=\"tag_id\" value=\"").unwrap() + 21..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    assert_ne!(first, last, "reset did not take");
}

/// A stranger cannot see a list's order, nor change it.
#[rstest]
#[tokio::test]
async fn a_stranger_cannot_reorder_my_tags(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Shop").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let (_, theirs) = signed_in(&pool, "google-oauth2|stranger").await;
    let (status, _) = get(&app, &format!("/lists/{list_id}/tags"), &theirs).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Watching is a read, so it is authorised like one -- and the page that a watcher
/// re-reads is too.
#[rstest]
#[tokio::test]
async fn a_stranger_can_neither_watch_nor_re_read_my_list(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Groceries").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let (_, theirs) = signed_in(&pool, "google-oauth2|stranger").await;

    for uri in [
        format!("/lists/{list_id}/events"),
        format!("/lists/{list_id}/items"),
    ] {
        let (status, _) = get(&app, &uri, &theirs).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} leaked");
    }
}

#[rstest]
#[tokio::test]
async fn a_list_can_be_renamed(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|renamer").await;
    post(&app, "/lists", &cookie, "name=Groceries").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    assert!(
        body.contains("value=\"Groceries\""),
        "the rename form is not prefilled: {body}"
    );

    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/rename"),
            &cookie,
            "name=Weekly+shop"
        )
        .await,
        StatusCode::SEE_OTHER
    );

    let (_, body) = get(&app, "/", &cookie).await;
    assert!(body.contains("Weekly shop"), "{body}");
    assert!(
        !body.contains("Groceries"),
        "the old name is still on the page"
    );
}

#[rstest]
#[tokio::test]
async fn an_item_can_be_edited(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|editor").await;
    post(&app, "/lists", &cookie, "name=Dairy").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Milk",
    )
    .await;

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    assert!(
        body.contains("value=\"Milk\""),
        "the edit form is not prefilled: {body}"
    );

    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/items/{item_id}/edit"),
            &cookie,
            "name=Oat+milk&amount=2&unit_id="
        )
        .await,
        StatusCode::SEE_OTHER
    );

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(body.contains("Oat milk"), "{body}");
    assert!(!body.contains(">Milk<"), "the old name is still shown");
    assert!(
        body.contains("value=\"2\""),
        "the amount did not change: {body}"
    );
}

/// Editing keeps the item where it is and does not tick it off.
#[rstest]
#[tokio::test]
async fn editing_leaves_the_rest_of_the_item_alone(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|careful").await;
    post(&app, "/lists", &cookie, "name=Bakery").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=6+Rolls",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    post(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/toggle"),
        &cookie,
        "",
    )
    .await;

    post(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/edit"),
        &cookie,
        "name=Bread+rolls&amount=6&unit_id=",
    )
    .await;

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(body.contains("Bread rolls"));
    assert!(body.contains("☑"), "editing un-ticked the item: {body}");
}

#[rstest]
#[case::rename_a_list("/lists/{list}/rename", "name=theirs+now")]
#[case::edit_an_item("/lists/{list}/items/{item}/edit", "name=theirs&amount=1&unit_id=")]
#[tokio::test]
async fn a_stranger_cannot_edit_my_things(
    #[future(awt)] pool: SqlitePool,
    #[case] path: &str,
    #[case] form: &str,
) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &mine,
        "line=Thing",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &mine).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|stranger").await;
    let uri = path
        .replace("{list}", &list_id.to_string())
        .replace("{item}", &item_id.to_string());

    assert_eq!(
        post(&app2, &uri, &theirs, form).await,
        StatusCode::NOT_FOUND
    );

    // and nothing changed
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &mine).await;
    assert!(body.contains("Thing"), "the item was edited by a stranger");
}

/// A fragment, not a page: no doctype, no header, no add form — just the part that
/// changed. Getting this wrong is how htmx ends up nesting a whole page inside a div.
fn is_fragment(body: &str, id: &str) -> bool {
    body.contains(&format!("id=\"{id}\""))
        && !body.contains("<!DOCTYPE")
        && !body.contains("<header")
}

#[rstest]
#[tokio::test]
async fn htmx_gets_a_fragment_where_a_browser_gets_a_redirect(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|htmx").await;

    // the plain form post is unchanged: still a redirect
    assert_eq!(
        post(&app, "/lists", &cookie, "name=Plain").await,
        StatusCode::SEE_OTHER,
        "the no-JavaScript path must keep working"
    );

    // the same route, announced as htmx, answers with the fragment
    let (status, body) = post_htmx(&app, "/lists", &cookie, "name=Dynamic").await;
    assert_eq!(status, StatusCode::OK);
    assert!(is_fragment(&body, "lists"), "not a fragment: {body}");
    assert!(
        body.contains("Dynamic"),
        "the new list is not in the swap: {body}"
    );
    assert!(
        body.contains("Plain"),
        "the swap must carry the whole list, not just the new row"
    );
}

#[rstest]
#[tokio::test]
async fn htmx_item_actions_return_the_board(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|htmx-items").await;
    post(&app, "/lists", &cookie, "name=Bakery").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);

    // add
    let (status, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=6+Bagels",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(is_fragment(&body, "items"), "not a fragment: {body}");
    assert!(body.contains("Bagels") && body.contains("☐"));

    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();

    // tick off -- the swap shows the new state without another request
    let (status, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/toggle"),
        &cookie,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(is_fragment(&body, "items"));
    assert!(
        body.contains("☑"),
        "the toggle did not come back ticked: {body}"
    );

    // delete
    let (status, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/delete"),
        &cookie,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Nothing on this list yet"), "{body}");
}

/// htmx is a client-side convenience, not an authorisation path: the header must not
/// buy anything a plain post could not.
#[rstest]
#[tokio::test]
async fn htmx_does_not_bypass_ownership(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let (app2, theirs) = signed_in(&pool, "google-oauth2|stranger").await;

    let (status, _) = post_htmx(
        &app2,
        &format!("/lists/{list_id}/items"),
        &theirs,
        "line=smuggled",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[rstest]
#[tokio::test]
async fn the_page_serves_its_own_htmx(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|assets").await;

    let (status, page) = get(&app, "/", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("/static/htmx.js"),
        "the page does not load htmx"
    );
    assert!(
        !page.contains("unpkg.com") && !page.contains("cdn."),
        "the page reaches a CDN: {page}"
    );

    let (status, js) = get(&app, "/static/htmx.js", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(js.contains("htmx"), "that is not htmx");
    assert!(
        js.len() > 10_000,
        "htmx looks truncated: {} bytes",
        js.len()
    );
}

/// Acting inside the panel swaps the whole board, so the markup that comes back
/// decides whether it is still open. Tagging keeps it — you usually add more than one
/// — while saving an edit closes it, because that finishes the job.
#[rstest]
#[tokio::test]
async fn the_panel_opens_and_closes_with_the_work(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|panel").await;
    post(&app, "/lists", &cookie, "name=Bakery").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Bagels",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    assert!(!panel_is_open(&body), "the panel starts closed");

    // saving finishes the job, so the panel closes behind it
    let (_, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/edit"),
        &cookie,
        "name=Sourdough&amount=1&unit_id=",
    )
    .await;
    assert!(body.contains("Sourdough"), "the edit did not take: {body}");
    assert!(!panel_is_open(&body), "saving left the panel open: {body}");

    // tagging does not: people add two or three at a time
    let (_, page) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let tag_id = first_tag_option(&page).expect("no tag options");
    let (_, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/tags"),
        &cookie,
        &format!("tag_id={tag_id}"),
    )
    .await;
    assert!(
        panel_is_open(&body),
        "the panel closed under the tag add: {body}"
    );

    // and removing one keeps it open too
    let (_, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/tags/{tag_id}/delete"),
        &cookie,
        "",
    )
    .await;
    assert!(
        panel_is_open(&body),
        "the panel closed under the tag removal: {body}"
    );

    // ticking off is done from the collapsed row, so it must NOT open anything
    let (_, body) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/toggle"),
        &cookie,
        "",
    )
    .await;
    assert!(
        !panel_is_open(&body),
        "ticking an item opened its panel: {body}"
    );
}

/// Editing replaces the item rather than appearing beneath it, and there is a way
/// back that is not "save" or "delete".
///
/// What CSS does with these cannot be asserted from here — no layout runs — but their
/// presence and wiring can: the row, the editor and the switch that chooses between
/// them all have to be siblings, and Cancel has to point at that switch.
#[rstest]
#[tokio::test]
async fn an_item_being_edited_offers_a_way_out(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|cancel").await;
    post(&app, "/lists", &cookie, "name=Bakery").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Bagels",
    )
    .await;

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    let switch = format!("panel-{item_id}");

    // the switch, the row and the editor are siblings, in that order
    let at_switch = body
        .find(&format!("id=\"{switch}\""))
        .expect("no panel switch");
    let at_view = body.find("class=\"view\"").expect("no item row");
    let at_editor = body.find("class=\"panel-body\"").expect("no editor");
    assert!(
        at_switch < at_view && at_view < at_editor,
        "the switch must precede both for the sibling selectors to reach them"
    );

    // the toggle and Cancel both point at that switch, so either can flip it
    assert!(
        body.contains(&format!("class=\"panel-toggle\" for=\"{switch}\"")),
        "no edit toggle wired to the switch: {body}"
    );
    assert!(
        body.contains(&format!("class=\"cancel\" for=\"{switch}\"")),
        "no Cancel wired to the switch: {body}"
    );

    // Cancel is a label, not a submit -- it must not be able to save anything
    let cancel_at = body.find("class=\"cancel\"").unwrap();
    let tag_start = body[..cancel_at].rfind('<').unwrap();
    assert!(
        body[tag_start..cancel_at].starts_with("<label"),
        "Cancel is not a label, so it would submit the form: {}",
        &body[tag_start..cancel_at + 40]
    );
}

/// Choosing a tag is the action. There is no confirm button to click, except for
/// browsers that cannot post without one — which is exactly what <noscript> means.
#[rstest]
#[tokio::test]
async fn choosing_a_tag_adds_it(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|picker").await;
    post(&app, "/lists", &cookie, "name=Bakery").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Bagels",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    let item_id: i64 = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();

    // the picker posts on change rather than on submit
    let form_at = body
        .find(&format!("action=\"/lists/{list_id}/items/{item_id}/tags\""))
        .unwrap();
    let form_end = body[form_at..].find('>').unwrap() + form_at;
    assert!(
        body[form_at..form_end].contains("hx-trigger=\"change\""),
        "the tag picker still waits to be submitted: {}",
        &body[form_at..form_end]
    );

    // the only Add button left is the one browsers without scripting need
    let tag_form_end = body[form_at..].find("</form>").unwrap() + form_at;
    let tag_form = &body[form_at..tag_form_end];
    assert!(
        !tag_form.contains("<button") || tag_form.contains("<noscript><button"),
        "there is a confirm button outside <noscript>: {tag_form}"
    );

    // and choosing one really does attach it
    let tag_id = first_tag_option(&body).expect("no tag options");
    let (_, after) = post_htmx(
        &app,
        &format!("/lists/{list_id}/items/{item_id}/tags"),
        &cookie,
        &format!("tag_id={tag_id}"),
    )
    .await;
    assert!(
        after.contains("class=\"chip removable\""),
        "the tag was not attached: {after}"
    );
    // the picker comes back offering what is left, without the one just added
    assert_ne!(
        first_tag_option(&after),
        Some(tag_id),
        "the picker still offers a tag already on the item"
    );
}

// ------------------------------------------------------- quick add & grouping

/// One field, read the way a person writes it.
#[rstest]
// Names come back capitalised: that happens where they are stored, so every client
// shows the same spelling rather than each deciding for itself.
#[case::just_a_name("line=Milk", "Milk", Some("1 unit"))]
#[case::amount_and_unit("line=2+kg+apples", "Apples", Some("2 kg"))]
#[case::no_space("line=500g+flour", "Flour", Some("500 g"))]
#[case::bare_amount("line=6+eggs", "Eggs", Some("6 unit"))]
#[case::spelled_deliberately("line=iPhone+charger", "iPhone charger", Some("1 unit"))]
#[tokio::test]
async fn quick_add_reads_the_line(
    #[with(fixtures::UNITS)]
    #[future(awt)]
    pool: SqlitePool,
    #[case] form: &str,
    #[case] name: &str,
    #[case] measure: Option<&str>,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|quick").await;
    post(&app, "/lists", &cookie, "name=Shop").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);

    assert_eq!(
        post(&app, &format!("/lists/{list_id}/items"), &cookie, form).await,
        StatusCode::SEE_OTHER
    );

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    // Matched inside the row's own element, not anywhere on the page: the add
    // field's placeholder contains "2 kg apples", and a bare `contains` was passing
    // on that rather than on the item.
    assert!(
        body.contains(&format!("class=\"grow\">{name}")),
        "{form} did not yield {name}: {body}"
    );
    match measure {
        Some(m) => assert!(
            body.contains(&format!("class=\"amount\">{m}<")),
            "{form} should show {m}: {body}"
        ),
        // Only a row with no unit at all, which nothing adds any more -- every item
        // gets `unit` if it is given nothing else. Kept so the arm exists if a row
        // from before that rule turns up.
        None => assert!(
            !body.contains("class=\"amount\""),
            "a row with no unit printed an amount: {body}"
        ),
    }
}

/// The quick-add field offers what this person has bought before, and nobody else's.
#[rstest]
#[tokio::test]
async fn the_history_is_my_own(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|shopper-a").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/", &mine).await;
    let my_list = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{my_list}/items"),
        &mine,
        "line=Sourdough",
    )
    .await;

    let (app2, theirs) = signed_in(&pool, "google-oauth2|shopper-b").await;
    post(&app2, "/lists", &theirs, "name=Theirs").await;
    let (_, body) = get(&app2, "/", &theirs).await;
    let their_list = first_list_id(&body);

    // Typed into my own list, it is offered.
    let (status, offered) = get(
        &app,
        &format!("/lists/{my_list}/suggestions?line=sour"),
        &mine,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        offered.contains("<option value=\"Sourdough\">"),
        "my own history is not offered: {offered}"
    );

    // On somebody else's list, it is not -- their list has its own memory.
    let (_, theirs_offered) = get(
        &app2,
        &format!("/lists/{their_list}/suggestions?line=sour"),
        &theirs,
    )
    .await;
    assert!(
        !theirs_offered.contains("Sourdough"),
        "my shopping leaked into someone else's suggestions: {theirs_offered}"
    );

    // And asking about my list from their session is a 404, not a peek.
    let (status, _) = get(
        &app,
        &format!("/lists/{my_list}/suggestions?line=sour"),
        &theirs,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Nothing typed, nothing offered. A datalist handed over in full is shown in full
/// the moment the field is focused, which is a second list on top of the real one.
#[rstest]
#[tokio::test]
async fn suggestions_wait_until_something_is_typed(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|patient").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &mine,
        "line=Sourdough",
    )
    .await;

    let (_, page) = get(&app, &format!("/lists/{list_id}"), &mine).await;
    assert!(
        !page.contains("<option value=\"Sourdough\">"),
        "the page arrived with the whole history in it: {page}"
    );

    for uri in [
        format!("/lists/{list_id}/suggestions"),
        format!("/lists/{list_id}/suggestions?line="),
        format!("/lists/{list_id}/suggestions?line=%20%20"),
    ] {
        let (status, offered) = get(&app, &uri, &mine).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!offered.contains("<option"), "{uri} offered something");
    }
}

/// Typed letters need not be the start of the word, or even next to each other --
/// the matching is the service's, so this is the browser reaching the same rules the
/// phone does.
#[rstest]
#[tokio::test]
async fn suggestions_are_matched_loosely(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|fuzzy").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &mine,
        "line=Sourdough",
    )
    .await;

    for typed in ["srdgh", "dough", "SOUR"] {
        let (_, offered) = get(
            &app,
            &format!("/lists/{list_id}/suggestions?line={typed}"),
            &mine,
        )
        .await;
        assert!(
            offered.contains("Sourdough"),
            "{typed:?} did not find it: {offered}"
        );
    }

    let (_, offered) = get(
        &app,
        &format!("/lists/{list_id}/suggestions?line=bread"),
        &mine,
    )
    .await;
    assert!(!offered.contains("<option"), "matched something unrelated");
}

/// Items sit under their category, and the categories run in shop order rather than
/// alphabetically — produce before dairy before frozen.
#[rstest]
#[tokio::test]
async fn the_list_is_grouped_in_shop_order(
    #[with(fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|walker").await;
    post(&app, "/lists", &cookie, "name=Weekly").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);

    // added in the wrong order on purpose
    for line in ["line=peas", "line=milk", "line=apples"] {
        post(&app, &format!("/lists/{list_id}/items"), &cookie, line).await;
    }
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;

    // Each item renders several URLs containing its id — toggle, edit, delete, tags —
    // so take the distinct ones in the order they first appear.
    let mut ids: Vec<i64> = Vec::new();
    for (at, _) in body.match_indices("/items/") {
        let id: i64 = body[at + 7..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    assert_eq!(ids.len(), 3, "expected three items, found {ids:?}");
    let (peas, milk, apples) = (ids[0], ids[1], ids[2]);

    let tag = |name: &str| -> i64 {
        let at = body
            .find(&format!(">{name}</option>"))
            .expect("tag missing");
        let v = body[..at].rfind("value=\"").unwrap() + 7;
        body[v..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap()
    };
    // frozen sorts last of the three, produce first
    for (item, tag_name) in [
        (peas, "🧊 frozen"),
        (milk, "🧀 dairy"),
        (apples, "🥬 produce"),
    ] {
        assert_eq!(
            post(
                &app,
                &format!("/lists/{list_id}/items/{item}/tags"),
                &cookie,
                &format!("tag_id={}", tag(tag_name)),
            )
            .await,
            StatusCode::SEE_OTHER,
            "could not tag {item} with {tag_name}"
        );
    }

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    // Look for the headings, not the words: every item's tag picker lists all 21 tag
    // names alphabetically, so a bare search finds those instead.
    let heading = |name: &str| {
        body.find(&format!("class=\"group-heading\">🥬 {name}<"))
            .or_else(|| body.find(&format!("class=\"group-heading\">🧀 {name}<")))
            .or_else(|| body.find(&format!("class=\"group-heading\">🧊 {name}<")))
            .unwrap_or_else(|| panic!("no {name} group in: {body}"))
    };
    let produce = heading("produce");
    let dairy = heading("dairy");
    let frozen = heading("frozen");
    assert!(
        produce < dairy && dairy < frozen,
        "groups are not in shop order: produce {produce}, dairy {dairy}, frozen {frozen}"
    );
}

/// Ticked items are collected out of the way, counted, and clearable in one go.
#[rstest]
#[tokio::test]
async fn done_items_collect_and_clear(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|tidy").await;
    post(&app, "/lists", &cookie, "name=Shop").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);
    for line in ["line=Milk", "line=Bread"] {
        post(&app, &format!("/lists/{list_id}/items"), &cookie, line).await;
    }
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(
        !body.contains("<details class=\"done-drawer\""),
        "nothing is done yet: {body}"
    );

    let first = body[body.find("/items/").unwrap() + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<i64>()
        .unwrap();
    post(
        &app,
        &format!("/lists/{list_id}/items/{first}/toggle"),
        &cookie,
        "",
    )
    .await;

    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(
        body.contains("<details class=\"done-drawer\""),
        "the done drawer is missing: {body}"
    );
    assert!(body.contains("1 done"), "the count is wrong: {body}");

    assert_eq!(
        post(&app, &format!("/lists/{list_id}/clear-done"), &cookie, "").await,
        StatusCode::SEE_OTHER
    );
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(
        !body.contains("<details class=\"done-drawer\""),
        "clearing left the drawer: {body}"
    );
    assert!(
        body.contains("Bread"),
        "clearing removed an outstanding item: {body}"
    );
}

#[rstest]
#[tokio::test]
async fn a_stranger_cannot_clear_my_list(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let (app2, theirs) = signed_in(&pool, "google-oauth2|stranger").await;

    assert_eq!(
        post(&app2, &format!("/lists/{list_id}/clear-done"), &theirs, "").await,
        StatusCode::NOT_FOUND
    );
}

// ------------------------------------------------------------------ security

/// The second layer under `SameSite=Lax`: a state-changing request that says it came
/// from another site is refused, whatever cookie it carries.
#[rstest]
#[case::a_hostile_origin("origin", "https://evil.example", StatusCode::FORBIDDEN)]
#[case::cross_site_fetch("sec-fetch-site", "cross-site", StatusCode::FORBIDDEN)]
#[case::our_own_origin("origin", "http://localhost:8080", StatusCode::SEE_OTHER)]
#[case::same_site_fetch("sec-fetch-site", "same-origin", StatusCode::SEE_OTHER)]
#[case::a_typed_url("sec-fetch-site", "none", StatusCode::SEE_OTHER)]
#[tokio::test]
async fn a_cross_site_post_is_refused(
    #[future(awt)] pool: SqlitePool,
    #[case] header_name: &str,
    #[case] header_value: &str,
    #[case] expected: StatusCode,
) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|victim").await;

    let res = app
        .oneshot(
            Request::builder()
                .uri("/lists")
                .method("POST")
                .header(header::COOKIE, cookie)
                .header(header_name, header_value)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("name=planted"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), expected, "{header_name}: {header_value}");
}

/// Reading is never refused — a link from anywhere still works.
#[rstest]
#[tokio::test]
async fn a_cross_site_read_is_allowed(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|reader").await;

    let res = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie)
                .header("sec-fetch-site", "cross-site")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

/// A page that needs `unsafe-inline` has a policy in name only, so the markup must
/// carry no inline script or style for the header to be worth sending.
#[rstest]
#[tokio::test]
async fn the_markup_has_nothing_a_strict_csp_would_block(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|csp").await;
    post(&app, "/lists", &cookie, "name=Shop").await;
    let (_, body) = get(&app, "/", &cookie).await;

    assert!(
        !body.contains("<style"),
        "an inline stylesheet remains: {body}"
    );
    assert!(
        !body.contains("hx-on"),
        "an inline event handler remains, which needs unsafe-inline: {body}"
    );
    assert!(
        body.contains("/static/app.css"),
        "the stylesheet is not served"
    );
    assert!(
        body.contains("/static/app.js"),
        "the behaviour is not served"
    );
}

/// A page that shows a prefix of a long list has to say so, or the missing items look
/// deleted rather than merely elsewhere.
#[rstest]
#[tokio::test]
async fn a_truncated_list_admits_it(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|hoarder").await;
    post(&app, "/lists", &cookie, "name=Big").await;
    let (_, body) = get(&app, "/", &cookie).await;
    let list_id = first_list_id(&body);

    // a short list says nothing
    post(
        &app,
        &format!("/lists/{list_id}/items"),
        &cookie,
        "line=Milk",
    )
    .await;
    let (_, body) = get(&app, &format!("/lists/{list_id}"), &cookie).await;
    assert!(
        !body.contains("class=\"truncated\""),
        "a one-item list claimed to be truncated"
    );
}

/// Every page reads from one ceiling rather than inventing its own.
#[test]
fn the_page_cap_has_one_definition() {
    assert_eq!(
        super::super::pages::items::page_cap(),
        domain::service::PAGE_MAX,
        "the items page has drifted from the shared ceiling"
    );
}

// ------------------------------------------------------------------- sharing

/// The whole flow as a person does it: create a link, follow it, and find the list
/// on the other person's screen.
#[rstest]
#[tokio::test]
async fn a_list_can_be_shared_by_link(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner").await;
    post(&app, "/lists", &mine, "name=Household").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    // sharing is offered from the index, beside renaming and deleting
    let (_, index) = get(&app, "/", &mine).await;
    assert!(
        index.contains(&format!("/lists/{list_id}/share")),
        "no way to reach sharing from the lists page: {index}"
    );

    let (_, page) = get(&app, &format!("/lists/{list_id}/share"), &mine).await;
    assert!(
        page.contains("Create an invitation link"),
        "no way to share: {page}"
    );

    let shown = post_page(&app, &format!("/lists/{list_id}/invites"), &mine, "").await;
    let at = shown.find("/join#").expect("no link on the page");
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    assert_eq!(token.len(), 64, "the token is not 256 bits of hex: {token}");
    assert!(
        shown.contains("only time it is shown"),
        "the page does not say the link cannot be recovered"
    );

    // somebody else follows it
    let (app2, theirs) = signed_in(&pool, "google-oauth2|housemate").await;
    let status = follow(&app2, &theirs, &token).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    // and now has the list
    let (_, their_page) = get(&app2, "/", &theirs).await;
    assert!(
        their_page.contains("Household"),
        "the shared list is missing: {their_page}"
    );

    // they can add to it, and the owner sees what they added
    assert_eq!(
        post(
            &app2,
            &format!("/lists/{list_id}/items"),
            &theirs,
            "line=2+kg+apples"
        )
        .await,
        StatusCode::SEE_OTHER
    );
    let (_, owner_page) = get(&app, &format!("/lists/{list_id}"), &mine).await;
    assert!(
        owner_page.contains("apples"),
        "the editor's item is not on the list"
    );
}

/// A viewer is given a list to read, not a list covered in controls that would refuse
/// them.
///
/// Only editor links are offered in the interface, so the viewer here is made through
/// the service — the role exists and the page has to render it honestly even though
/// nothing in the UI hands it out yet.
#[rstest]
#[tokio::test]
async fn a_viewer_is_not_offered_the_controls(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner2").await;
    post(&app, "/lists", &mine, "name=Readonly").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    post(&app, &format!("/lists/{list_id}/items"), &mine, "line=Milk").await;

    let ctx = Ctx::new(pool.clone());
    let owner = domain::service::Actor::User(
        domain::models::user::User::find_or_create(
            &pool,
            Sub("google-oauth2|owner2".into()),
            None,
            None,
        )
        .await
        .unwrap(),
    );
    let token = domain::service::lists::invite(
        &ctx,
        &owner,
        domain::models::list::Id(list_id),
        domain::models::list::Role::Viewer,
    )
    .await
    .unwrap();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|looker").await;
    follow(&app2, &theirs, &token.0).await;

    let (status, page) = get(&app2, &format!("/lists/{list_id}"), &theirs).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Milk"),
        "a viewer cannot see the list: {page}"
    );

    for control in [
        "Add an item",
        "Remove item",
        "panel-toggle",
        "class=\"tick\"",
    ] {
        assert!(
            !page.contains(control),
            "a viewer was offered {control}, which would refuse them: {page}"
        );
    }

    // and the refusal, if they contrive one, says forbidden rather than missing
    assert_eq!(
        post(
            &app2,
            &format!("/lists/{list_id}/items"),
            &theirs,
            "line=smuggled"
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

/// An editor gets the controls, and the owner's are still the owner's.
#[rstest]
#[tokio::test]
async fn an_editor_is_offered_the_controls_but_not_the_owners(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner4").await;
    post(&app, "/lists", &mine, "name=Shared").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    post(&app, &format!("/lists/{list_id}/items"), &mine, "line=Milk").await;

    let shown = post_page(&app, &format!("/lists/{list_id}/invites"), &mine, "").await;
    let at = shown.find("/join#").unwrap();
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|helper").await;
    follow(&app2, &theirs, &token).await;

    let (_, page) = get(&app2, &format!("/lists/{list_id}"), &theirs).await;
    assert!(page.contains("Add an item"), "an editor cannot add: {page}");
    assert!(page.contains("Remove item"), "an editor cannot remove");

    let (_, share) = get(&app2, &format!("/lists/{list_id}/share"), &theirs).await;
    assert!(
        !share.contains("Create an invitation link"),
        "someone who is not the owner was offered sharing: {share}"
    );
    assert!(
        share.contains("Leave this list"),
        "no way out for a member: {share}"
    );
}

/// A stranger is still told nothing at all.
#[rstest]
#[tokio::test]
async fn a_stranger_still_sees_a_404(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|owner3").await;
    post(&app, "/lists", &mine, "name=Private").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let (app2, theirs) = signed_in(&pool, "google-oauth2|nobody").await;

    let (status, _) = get(&app2, &format!("/lists/{list_id}"), &theirs).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        post(&app2, &format!("/lists/{list_id}/items"), &theirs, "line=x").await,
        StatusCode::NOT_FOUND,
        "a stranger was told the list exists"
    );
}

/// A guessed link is a miss, and the token never appears in storage in the clear.
#[rstest]
#[tokio::test]
async fn a_guessed_link_gets_nowhere(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|guesser").await;

    let status = follow(&app, &cookie, "0000000000000000").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The token is after the `#`, and nowhere a server could write it down.
///
/// This is the whole reason the link has that shape. A path is logged by every proxy,
/// reverse proxy and access log between a phone and somebody's home server, and a
/// share token stays valid for a week. A fragment is never sent at all.
#[rstest]
#[tokio::test]
async fn a_share_link_keeps_its_token_out_of_the_path(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|host").await;
    post(&app, "/lists", &mine, "name=Household").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);

    let shown = post_page(&app, &format!("/lists/{list_id}/invites"), &mine, "").await;

    let at = shown.find("/join#").expect("no link on the page");
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    assert_eq!(token.len(), 64, "the token is not 256 bits of hex: {token}");
    assert!(
        !shown.contains(&format!("/join/{token}")),
        "the token is in the path as well, which defeats the fragment: {shown}"
    );
}

/// Following a link on a device nobody has signed in on yet.
///
/// The invitation has to survive the trip to Google and back, and it cannot make that
/// trip in the fragment -- the browser drops it the moment it leaves the page. So it
/// goes into the session instead. Losing it here means somebody who was sent a link
/// signs in, lands on their own empty list of lists, and has nothing left to try.
#[rstest]
#[tokio::test]
async fn a_link_followed_before_signing_in_is_not_lost(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|host").await;
    post(&app, "/lists", &mine, "name=Household").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    let shown = post_page(&app, &format!("/lists/{list_id}/invites"), &mine, "").await;
    let at = shown.find("/join#").unwrap();
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();

    // No cookie: a stranger, on a phone that has never signed in.
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/join")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("token={token}")))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        res.headers().get(header::LOCATION).unwrap(),
        "/auth/login",
        "a signed-out visitor was not sent to sign in"
    );
    assert!(
        res.headers().get(header::SET_COOKIE).is_some(),
        "nothing was kept, so the invitation is gone once they come back"
    );
}

/// Without scripting there is nobody to read the address bar, so the whole link is
/// pasted. The part after the `#` is the token.
#[rstest]
#[tokio::test]
async fn a_pasted_link_is_accepted_whole(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|host").await;
    post(&app, "/lists", &mine, "name=Household").await;
    let (_, body) = get(&app, "/", &mine).await;
    let list_id = first_list_id(&body);
    let shown = post_page(&app, &format!("/lists/{list_id}/invites"), &mine, "").await;
    let at = shown.find("/join#").unwrap();
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|housemate").await;
    let status = follow(
        &app2,
        &theirs,
        &format!("https://example.com/join%23{token}"),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, their_page) = get(&app2, "/", &theirs).await;
    assert!(
        their_page.contains("Household"),
        "a pasted link did not work: {their_page}"
    );
}

// ------------------------------------------------------- coming back to it

/// Following a share link, the way the page at `/join` does.
///
/// A post rather than a GET of a path: the token travels in the fragment, which never
/// reaches the server, so the browser reads it out of its own address bar and hands it
/// back in a form body. See `pages::sharing::joining`.
async fn follow(app: &axum::Router, cookie: &str, token: &str) -> StatusCode {
    post(app, "/join", cookie, &format!("token={token}")).await
}

/// Returns the Location of a redirect, or None if the response was a page.
async fn location(app: &axum::Router, uri: &str, cookie: &str) -> Option<String> {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    res.headers()
        .get(header::LOCATION)
        .map(|v| v.to_str().unwrap().to_string())
}

/// The application comes back up where it was left, rather than on a menu you then
/// have to navigate out of again.
#[rstest]
#[tokio::test]
async fn it_opens_where_you_left_off(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|returner").await;
    post(&app, "/lists", &cookie, "name=Weekly").await;
    post(&app, "/lists", &cookie, "name=Hardware").await;
    let (_, body) = get(&app, "/lists", &cookie).await;
    let a_list = first_list_id(&body);

    // nothing has been opened yet, so the home page is the index
    assert_eq!(location(&app, "/", &cookie).await, None);

    get(&app, &format!("/lists/{a_list}"), &cookie).await;

    assert_eq!(
        location(&app, "/", &cookie).await,
        Some(format!("/lists/{a_list}")),
        "it did not come back to the list that was open"
    );

    // and the index is still reachable, or the home link would bounce you back
    let (status, index) = get(&app, "/lists", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(index.contains("Weekly") && index.contains("Hardware"));
}

/// Somewhere that no longer exists is not somewhere to come back to.
#[rstest]
#[tokio::test]
async fn a_deleted_list_is_forgotten(#[future(awt)] pool: SqlitePool) {
    let (app, cookie) = signed_in(&pool, "google-oauth2|deleter").await;
    post(&app, "/lists", &cookie, "name=Doomed").await;
    let (_, body) = get(&app, "/lists", &cookie).await;
    let id = first_list_id(&body);
    get(&app, &format!("/lists/{id}"), &cookie).await;
    assert!(location(&app, "/", &cookie).await.is_some());

    post(&app, &format!("/lists/{id}/delete"), &cookie, "").await;

    assert_eq!(
        location(&app, "/", &cookie).await,
        None,
        "it tried to return to a deleted list"
    );
}

/// Nor is somewhere access was taken away.
#[rstest]
#[tokio::test]
async fn a_list_you_were_removed_from_is_forgotten(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|sharer").await;
    post(&app, "/lists", &mine, "name=Household").await;
    let (_, body) = get(&app, "/lists", &mine).await;
    let id = first_list_id(&body);

    let shown = post_page(&app, &format!("/lists/{id}/invites"), &mine, "").await;
    let at = shown.find("/join#").unwrap();
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|guest").await;
    follow(&app2, &theirs, &token).await;
    get(&app2, &format!("/lists/{id}"), &theirs).await;
    assert!(
        location(&app2, "/", &theirs).await.is_some(),
        "they were not there"
    );

    // the owner takes it back
    let members: Vec<i64> = vec![];
    let _ = members;
    let (_, share) = get(&app, &format!("/lists/{id}/share"), &mine).await;
    let at = share.find("/members/").expect("no member to remove");
    let who: i64 = share[at + 9..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap();
    post(
        &app,
        &format!("/lists/{id}/members/{who}/remove"),
        &mine,
        "",
    )
    .await;

    assert_eq!(
        location(&app2, "/", &theirs).await,
        None,
        "it tried to return to a list they can no longer open"
    );
}

/// Leaving a list forgets it, or the next visit sends you straight back in.
#[rstest]
#[tokio::test]
async fn leaving_a_list_forgets_it(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|host").await;
    post(&app, "/lists", &mine, "name=Shared").await;
    let (_, body) = get(&app, "/lists", &mine).await;
    let id = first_list_id(&body);
    let shown = post_page(&app, &format!("/lists/{id}/invites"), &mine, "").await;
    let at = shown.find("/join#").unwrap();
    let token: String = shown[at + 6..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();

    let (app2, theirs) = signed_in(&pool, "google-oauth2|leaver").await;
    follow(&app2, &theirs, &token).await;
    get(&app2, &format!("/lists/{id}"), &theirs).await;

    post(&app2, &format!("/lists/{id}/leave"), &theirs, "").await;

    assert_eq!(location(&app2, "/", &theirs).await, None);
}

/// A list somebody else opened is not where *this* person left off.
#[rstest]
#[tokio::test]
async fn where_you_left_off_is_yours(#[future(awt)] pool: SqlitePool) {
    let (app, mine) = signed_in(&pool, "google-oauth2|one").await;
    post(&app, "/lists", &mine, "name=Mine").await;
    let (_, body) = get(&app, "/lists", &mine).await;
    let id = first_list_id(&body);
    get(&app, &format!("/lists/{id}"), &mine).await;

    let (app2, theirs) = signed_in(&pool, "google-oauth2|two").await;

    assert_eq!(
        location(&app2, "/", &theirs).await,
        None,
        "one person's place was handed to another"
    );
}
