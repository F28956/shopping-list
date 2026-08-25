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
        body.contains("<option value=\"\">unit</option>"),
        "no unit picker: {body}"
    );

    // add an item with a unit
    assert_eq!(
        post(
            &app,
            &format!("/lists/{list_id}/items"),
            &cookie,
            "name=Apples&amount=2&unit_id="
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
        body.contains("class=\"done\""),
        "the row is not struck through"
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
            "name=smuggled"
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
        "name=Bagels&amount=1&unit_id=",
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

    // pick whatever the first option offers
    let opt = body.find("name=\"tag_id\"").expect("no tag select");
    let tag_id: i64 = body[body[opt..].find("value=\"").unwrap() + opt + 7..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("no tag options -- are tags seeded?");

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
    assert!(
        body.contains("class=\"chip\""),
        "the tag chip did not render: {body}"
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
        "name=Secret&amount=1&unit_id=",
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
        "name=Milk&amount=1&unit_id=",
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
        "name=Rolls&amount=6&unit_id=",
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
        "name=Thing&amount=1&unit_id=",
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
        "name=Bagels&amount=6&unit_id=",
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
        "name=smuggled&amount=1&unit_id=",
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
