//! The pages, driven through the real router with a real session.
//!
//! These are what verify the half of the application a person actually touches: that
//! a list renders, that ticking an item off changes what comes back, and that one
//! person's list is not on another person's screen.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use domain::models::pool;
use domain::models::user::{Name, Sub, User};
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
