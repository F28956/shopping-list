//! The rule the whole architecture rests on, checked once per operation.
//!
//! Every service function scoped to an owner must give a person who is not that owner
//! exactly the answer they would get for something that does not exist, and must
//! leave the row alone. These live together rather than beside each operation so that
//! the coverage is countable: if an operation is missing from this file, it has not
//! been checked.

use rstest::rstest;
use sqlx::SqlitePool;

use crate::models::pool;
use crate::models::{Direction, OrderBy, Paging};
use crate::models::{item, list, note, tag, unit};
use crate::service::tests::person;
use crate::service::{Actor, Ctx, ServiceError, items, lists, notes, tags, units, users};

fn all() -> Paging {
    Paging {
        number: 1,
        size: 100,
    }
}

fn order<F>(field: F) -> OrderBy<F> {
    OrderBy {
        field,
        direction: Direction::Ascending,
    }
}

/// Two people, and a list, item and note belonging to the first.
struct Scene {
    ctx: Ctx,
    mine: Actor,
    theirs: Actor,
    list: list::List,
    item: item::Item,
    note: note::Note,
}

async fn scene(pool: SqlitePool) -> Scene {
    let ctx = Ctx::new(pool.clone());
    let mine = person(&pool, "google-oauth2|owner").await;
    let theirs = person(&pool, "google-oauth2|stranger").await;

    let list = lists::create(&ctx, &mine, list::Name("Fruit & veg".into()))
        .await
        .unwrap();
    let item = items::create(
        &ctx,
        &mine,
        list.id,
        item::Name("Apples".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();
    let note = notes::create(&ctx, &mine, note::Body("remember the bags".into()))
        .await
        .unwrap();

    Scene {
        ctx,
        mine,
        theirs,
        list,
        item,
        note,
    }
}

// ------------------------------------------------------------------------ lists

#[rstest]
#[tokio::test]
async fn a_stranger_cannot_touch_a_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let (ctx, them, id) = (&s.ctx, &s.theirs, s.list.id);

    assert_eq!(lists::get(ctx, them, id).await, Err(ServiceError::NotFound));
    assert_eq!(
        lists::update(ctx, them, id, list::Name("theirs now".into())).await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        lists::delete(ctx, them, id).await,
        Err(ServiceError::NotFound)
    );

    // and nothing happened to it
    let after = lists::get(ctx, &s.mine, id).await.unwrap();
    assert_eq!(after.name, s.list.name);
}

#[rstest]
#[tokio::test]
async fn a_strangers_list_is_not_in_my_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let page = lists::for_user(&s.ctx, &s.theirs, all(), order(list::Field::Id))
        .await
        .unwrap();

    assert_eq!(page.total, 0);
}

// ------------------------------------------------------------------------ items

#[rstest]
#[tokio::test]
async fn a_stranger_cannot_touch_an_item(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let (ctx, them, id) = (&s.ctx, &s.theirs, s.item.id);

    assert_eq!(items::get(ctx, them, id).await, Err(ServiceError::NotFound));
    assert_eq!(
        items::update(
            ctx,
            them,
            id,
            item::Name("theirs".into()),
            item::Amount(9.0),
            None
        )
        .await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        items::set_done(ctx, them, id, true).await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        items::delete(ctx, them, id).await,
        Err(ServiceError::NotFound)
    );

    let after = items::get(ctx, &s.mine, id).await.unwrap();
    assert_eq!(after, s.item, "the item is untouched");
}

/// An item id is not a capability: the list is what is consulted, so an id obtained
/// from somewhere else buys nothing.
#[rstest]
#[tokio::test]
async fn a_stranger_cannot_read_or_add_to_my_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    assert_eq!(
        items::for_list(&s.ctx, &s.theirs, s.list.id, all(), order(item::Field::Id))
            .await
            .err(),
        Some(ServiceError::NotFound)
    );
    assert_eq!(
        items::create(
            &s.ctx,
            &s.theirs,
            s.list.id,
            item::Name("smuggled".into()),
            item::Amount(1.0),
            None
        )
        .await
        .err(),
        Some(ServiceError::NotFound),
        "a stranger added an item to someone else's list"
    );

    let mine = items::for_list(&s.ctx, &s.mine, s.list.id, all(), order(item::Field::Id))
        .await
        .unwrap();
    assert_eq!(mine.total, 1, "nothing was smuggled in");
}

// ------------------------------------------------------------------------- tags

#[rstest]
#[tokio::test]
async fn a_stranger_cannot_tag_my_item(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let tag = tags::create(
        &s.ctx,
        &Actor::System,
        tag::Name("pantry".into()),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        tags::attach(&s.ctx, &s.theirs, s.item.id, tag.id).await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        tags::for_item(&s.ctx, &s.theirs, s.item.id).await.err(),
        Some(ServiceError::NotFound)
    );

    tags::attach(&s.ctx, &s.mine, s.item.id, tag.id)
        .await
        .unwrap();
    assert_eq!(
        tags::detach(&s.ctx, &s.theirs, s.item.id, tag.id).await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        tags::for_item(&s.ctx, &s.mine, s.item.id)
            .await
            .unwrap()
            .len(),
        1,
        "the tag a stranger tried to remove is still there"
    );
}

// ---------------------------------------------------------------- shared data

/// Units and tags are read by everyone and written by nobody but the process.
#[rstest]
#[tokio::test]
async fn a_person_may_read_but_not_write_shared_data(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let unit = units::create(&s.ctx, &Actor::System, unit::Name("kg".into()))
        .await
        .unwrap();

    // reading is fine
    units::list(&s.ctx, &s.mine, all(), order(unit::Field::Id))
        .await
        .unwrap();
    units::get(&s.ctx, &s.mine, unit::Lookup::Id(unit.id))
        .await
        .unwrap();

    // writing is not
    assert_eq!(
        units::create(&s.ctx, &s.mine, unit::Name("furlong".into()))
            .await
            .err(),
        Some(ServiceError::NotFound)
    );
    assert_eq!(
        units::update(&s.ctx, &s.mine, unit.id, unit::Name("kilo".into()))
            .await
            .err(),
        Some(ServiceError::NotFound)
    );
    assert_eq!(
        units::delete(&s.ctx, &s.mine, unit.id).await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        tags::create(&s.ctx, &s.mine, tag::Name("mine".into()), None, None)
            .await
            .err(),
        Some(ServiceError::NotFound)
    );

    // and the unit is still called what it was
    let after = units::get(&s.ctx, &s.mine, unit::Lookup::Id(unit.id))
        .await
        .unwrap();
    assert_eq!(after.name, unit.name);
}

// ------------------------------------------------------------------------ users

#[rstest]
#[tokio::test]
async fn a_person_sees_only_themselves(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let me = users::me(&s.ctx, &s.mine).await.unwrap();
    assert_eq!(me.id, s.mine.person().unwrap().id);

    assert_eq!(
        users::list(&s.ctx, &s.mine, all(), order(user_field()))
            .await
            .err(),
        Some(ServiceError::NotFound),
        "the user list is for maintenance, not for people"
    );
    assert!(
        users::list(&s.ctx, &Actor::System, all(), order(user_field()))
            .await
            .is_ok()
    );
}

fn user_field() -> crate::models::user::Field {
    crate::models::user::Field::Id
}

/// Closing an account takes everything with it, and only ever the actor's own.
#[rstest]
#[tokio::test]
async fn closing_an_account_takes_only_my_things(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    users::close_account(&s.ctx, &s.theirs).await.unwrap();

    // mine survives
    assert!(lists::get(&s.ctx, &s.mine, s.list.id).await.is_ok());
    assert!(items::get(&s.ctx, &s.mine, s.item.id).await.is_ok());
    assert!(notes::get(&s.ctx, &s.mine, s.note.id).await.is_ok());

    users::close_account(&s.ctx, &s.mine).await.unwrap();
    assert_eq!(
        lists::get(&s.ctx, &s.mine, s.list.id).await,
        Err(ServiceError::NotFound),
        "the list went with the account"
    );
}

// ---------------------------------------------------------------------- system

/// The system is not a person, so nothing owner-scoped will act for it.
#[rstest]
#[tokio::test]
async fn the_system_owns_nothing(#[future(awt)] pool: SqlitePool) {
    let ctx = Ctx::new(pool);
    let sys = Actor::System;

    assert_eq!(
        lists::create(&ctx, &sys, list::Name("whose?".into()))
            .await
            .err(),
        Some(ServiceError::Unauthenticated)
    );
    assert_eq!(
        lists::for_user(&ctx, &sys, all(), order(list::Field::Id))
            .await
            .err(),
        Some(ServiceError::Unauthenticated)
    );
    assert_eq!(
        items::get(&ctx, &sys, item::Id(1)).await.err(),
        Some(ServiceError::Unauthenticated)
    );
    assert_eq!(
        users::me(&ctx, &sys).await.err(),
        Some(ServiceError::Unauthenticated)
    );
}

// -------------------------------------------------------------- new surfaces

/// Suggestions come from the actor's own history and nobody else's.
#[rstest]
#[tokio::test]
async fn suggestions_are_my_own_history(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    // the other person buys something distinctive
    let theirs_list = lists::create(&s.ctx, &s.theirs, list::Name("Theirs".into()))
        .await
        .unwrap();
    items::create(
        &s.ctx,
        &s.theirs,
        theirs_list.id,
        item::Name("Absinthe".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    let mine = items::suggestions(&s.ctx, &s.mine, s.list.id, 50, None)
        .await
        .unwrap();

    assert!(
        mine.iter().any(|n| n.0 == "Apples"),
        "my own item is missing: {mine:?}"
    );
    assert!(
        !mine.iter().any(|n| n.0 == "Absinthe"),
        "another person's shopping leaked into my suggestions: {mine:?}"
    );
}

#[rstest]
#[tokio::test]
async fn clearing_done_items_needs_the_list_to_be_mine(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::set_done(&s.ctx, &s.mine, s.item.id, true)
        .await
        .unwrap();

    assert_eq!(
        items::clear_done(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::NotFound),
        "a stranger cleared someone else's list"
    );
    assert!(
        items::get(&s.ctx, &s.mine, s.item.id).await.is_ok(),
        "the item was deleted anyway"
    );

    assert_eq!(
        items::clear_done(&s.ctx, &s.mine, s.list.id).await.unwrap(),
        1
    );
    assert_eq!(
        items::get(&s.ctx, &s.mine, s.item.id).await,
        Err(ServiceError::NotFound)
    );
}

/// Only the ticked ones go.
#[rstest]
#[tokio::test]
async fn clearing_done_leaves_outstanding_items(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let still_needed = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        item::Name("Bananas".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();
    items::set_done(&s.ctx, &s.mine, s.item.id, true)
        .await
        .unwrap();

    let gone = items::clear_done(&s.ctx, &s.mine, s.list.id).await.unwrap();

    assert_eq!(gone, 1);
    assert!(items::get(&s.ctx, &s.mine, still_needed.id).await.is_ok());
}

/// Clearing a list with nothing ticked is a no-op, not an error: the button is
/// allowed to be pressed twice.
#[rstest]
#[tokio::test]
async fn clearing_nothing_is_not_an_error(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    assert_eq!(
        items::clear_done(&s.ctx, &s.mine, s.list.id).await.unwrap(),
        0
    );
}

// ------------------------------------------------------------------- history

use crate::models::history::{Entry, MAX_ENTRIES};

/// The reason the history table exists: it has to outlive the lists it was gathered
/// from. Deriving suggestions from live rows meant "clear done" — the natural
/// end-of-shop action — wiped the lot.
#[rstest]
#[tokio::test]
async fn history_survives_clearing_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, "Sourdough")
        .await
        .unwrap();
    let item = items::for_list(&s.ctx, &s.mine, s.list.id, all(), order(item::Field::Id))
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.name.0 == "Sourdough")
        .unwrap();
    items::set_done(&s.ctx, &s.mine, item.id, true)
        .await
        .unwrap();

    items::clear_done(&s.ctx, &s.mine, s.list.id).await.unwrap();

    let after = items::suggestions(&s.ctx, &s.mine, s.list.id, 50, None)
        .await
        .unwrap();
    assert!(
        after.iter().any(|n| n.0 == "Sourdough"),
        "clearing the list forgot what was on it: {after:?}"
    );
}

/// Deleting the list does take its history, and that is the price of keying it on the
/// list: a household shares one memory, and there is nowhere for it to live once the
/// list it belonged to is gone.
///
/// Tolerable only because this application pushes towards lists that live a long time
/// — "clear done" exists so the same list carries week to week. It would be the wrong
/// trade in an application where a shop means a new list.
#[rstest]
#[tokio::test]
async fn history_goes_with_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, "Rye")
        .await
        .unwrap();

    lists::delete(&s.ctx, &s.mine, s.list.id).await.unwrap();

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM item_history WHERE list_id = ?1")
        .bind(s.list.id.0)
        .fetch_one(&s.ctx.db)
        .await
        .unwrap();
    assert_eq!(rows, 0, "the history outlived the list it belonged to");
}

/// The payoff: an item added a second time arrives measured and filed, from four
/// letters and no other input.
#[rstest]
#[tokio::test]
async fn a_remembered_item_returns_measured_and_filed(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let pint = units::create(&s.ctx, &Actor::System, unit::Name("pint".into()))
        .await
        .unwrap();
    let dairy = tags::create(
        &s.ctx,
        &Actor::System,
        tag::Name("dairy".into()),
        None,
        None,
    )
    .await
    .unwrap();

    // first time: spelled out, then filed by hand
    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, "4 pint milk")
        .await
        .unwrap();
    assert_eq!(first.unit_id, Some(pint.id));
    tags::attach(&s.ctx, &s.mine, first.id, dairy.id)
        .await
        .unwrap();

    // second time: just the word
    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, "milk")
        .await
        .unwrap();

    assert_eq!(again.unit_id, Some(pint.id), "the unit was not remembered");
    assert_eq!(
        again.amount,
        item::Amount(1.0),
        "the amount is not remembered, only the unit"
    );
    let on_it = tags::for_item(&s.ctx, &s.mine, again.id).await.unwrap();
    assert_eq!(
        on_it.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![dairy.id],
        "the category was not remembered"
    );
}

/// A line that says a unit outranks the remembered one — this week is two litres,
/// whatever last week was.
#[rstest]
#[tokio::test]
async fn the_line_beats_the_memory(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let pint = units::create(&s.ctx, &Actor::System, unit::Name("pint".into()))
        .await
        .unwrap();
    let litre = units::create(&s.ctx, &Actor::System, unit::Name("litre".into()))
        .await
        .unwrap();
    items::quick_add(&s.ctx, &s.mine, s.list.id, "4 pint milk")
        .await
        .unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, "2 litre milk")
        .await
        .unwrap();

    assert_eq!(again.unit_id, Some(litre.id));
    assert_ne!(again.unit_id, Some(pint.id));
}

/// Unfiling is a signal too — stop putting it there.
#[rstest]
#[tokio::test]
async fn removing_a_tag_stops_it_coming_back(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let dairy = tags::create(
        &s.ctx,
        &Actor::System,
        tag::Name("dairy".into()),
        None,
        None,
    )
    .await
    .unwrap();
    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, "milk")
        .await
        .unwrap();
    tags::attach(&s.ctx, &s.mine, first.id, dairy.id)
        .await
        .unwrap();
    tags::detach(&s.ctx, &s.mine, first.id, dairy.id)
        .await
        .unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, "milk")
        .await
        .unwrap();

    assert!(
        tags::for_item(&s.ctx, &s.mine, again.id)
            .await
            .unwrap()
            .is_empty(),
        "a tag that was taken off came back"
    );
}

/// One memory per item however it is spelled.
#[rstest]
#[tokio::test]
async fn spelling_does_not_split_the_memory(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    items::quick_add(&s.ctx, &s.mine, s.list.id, "Milk")
        .await
        .unwrap();
    items::quick_add(&s.ctx, &s.mine, s.list.id, "MILK")
        .await
        .unwrap();
    items::quick_add(&s.ctx, &s.mine, s.list.id, "  milk ")
        .await
        .unwrap();

    let suggestions = items::suggestions(&s.ctx, &s.mine, s.list.id, 50, None)
        .await
        .unwrap();
    let milks: Vec<_> = suggestions
        .iter()
        .filter(|n| n.0.to_lowercase() == "milk")
        .collect();
    assert_eq!(
        milks.len(),
        1,
        "one item became several memories: {suggestions:?}"
    );
    // Shown back the way it was last stored, which is capitalised: names are
    // capitalised on the way in, so the memory carries the same spelling the list
    // does rather than a transcript of the last person's typing.
    assert_eq!(milks[0].0, "Milk");
}

/// A typo can be taken back.
#[rstest]
#[tokio::test]
async fn a_mistake_can_be_forgotten(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, "Mlik")
        .await
        .unwrap();

    items::forget(&s.ctx, &s.mine, s.list.id, item::Name("mlik".into()))
        .await
        .unwrap();

    let after = items::suggestions(&s.ctx, &s.mine, s.list.id, 50, None)
        .await
        .unwrap();
    assert!(!after.iter().any(|n| n.0 == "Mlik"), "{after:?}");
    // and forgetting something that was never there is a miss, not a silent no-op
    assert_eq!(
        items::forget(&s.ctx, &s.mine, s.list.id, item::Name("never".into())).await,
        Err(ServiceError::NotFound)
    );
}

/// A stranger is refused before a row is written for them.
///
/// The `find_or_create` on the far side of this call is what makes an account, so
/// checking after it would leave one behind for everybody who tried the door.
#[rstest]
#[tokio::test]
async fn an_unlisted_address_cannot_sign_in(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::admission::Admission;
    use crate::service::identity;

    let ctx = Ctx::with_admission(
        pool.clone(),
        Admission::parse("me@example.com").unwrap(),
    );

    assert_eq!(
        identity::from_claims(
            &ctx,
            user::Sub("google-oauth2|stranger".into()),
            Some(user::Name("Stranger".into())),
            Some(user::Email("stranger@example.com".into())),
        )
        .await
        .err(),
        Some(ServiceError::Forbidden)
    );

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "a refused sign-in must not create an account");

    // ... and the listed address still gets in.
    assert!(
        identity::from_claims(
            &ctx,
            user::Sub("google-oauth2|me".into()),
            Some(user::Name("Me".into())),
            Some(user::Email("Me@Example.com".into())),
        )
        .await
        .is_ok(),
        "however it is capitalised"
    );
}

/// Taking someone off the list ends the session they already hold. Checking only at
/// sign-in would mean removal did nothing until their cookie happened to expire.
#[rstest]
#[tokio::test]
async fn a_session_stops_working_when_the_address_is_removed(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::admission::Admission;
    use crate::service::identity;

    let welcome = Ctx::with_admission(pool.clone(), Admission::parse("me@example.com").unwrap());
    let actor = identity::from_claims(
        &welcome,
        user::Sub("google-oauth2|me".into()),
        Some(user::Name("Me".into())),
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();
    let id = actor.person().unwrap().id.0;

    assert!(identity::from_session(&welcome, id).await.unwrap().is_some());

    let removed = Ctx::with_admission(
        pool.clone(),
        Admission::parse("someone-else@example.com").unwrap(),
    );
    assert!(
        identity::from_session(&removed, id).await.unwrap().is_none(),
        "the session outlived the permission"
    );
}

/// Uncapped, every typo would live forever.
#[rstest]
#[tokio::test]
async fn history_is_capped(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    // one over the cap, each used once
    for n in 0..=MAX_ENTRIES {
        items::quick_add(&s.ctx, &s.mine, s.list.id, &format!("item-{n}"))
            .await
            .unwrap();
    }

    let held = Entry::for_list(&s.ctx.db, s.list.id, 10_000).await.unwrap();
    assert_eq!(held.len(), MAX_ENTRIES as usize, "the cap did not hold");
}

/// Another person's history is not mine, however much we shop alike.
#[rstest]
#[tokio::test]
async fn history_is_private(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let theirs_list = lists::create(&s.ctx, &s.theirs, list::Name("Theirs".into()))
        .await
        .unwrap();
    items::quick_add(&s.ctx, &s.theirs, theirs_list.id, "Absinthe")
        .await
        .unwrap();

    let mine = items::suggestions(&s.ctx, &s.mine, s.list.id, 50, None)
        .await
        .unwrap();

    assert!(!mine.iter().any(|n| n.0 == "Absinthe"), "{mine:?}");
    assert_eq!(
        items::forget(&s.ctx, &s.mine, s.list.id, item::Name("absinthe".into())).await,
        Err(ServiceError::NotFound),
        "one person forgot another person's history"
    );
}

// ------------------------------------------------------------------- sharing

use crate::models::list::Role;

/// Puts `theirs` on `mine`'s list at `role`, the way a person would: an invitation,
/// then a link followed.
async fn share(s: &Scene, role: Role) {
    let token = lists::invite(&s.ctx, &s.mine, s.list.id, role)
        .await
        .unwrap();
    lists::join(&s.ctx, &s.theirs, &token).await.unwrap();
}

#[rstest]
#[tokio::test]
async fn a_shared_list_appears_for_the_person_it_was_shared_with(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let before = lists::for_user(&s.ctx, &s.theirs, all(), order(list::Field::Id))
        .await
        .unwrap();
    assert_eq!(before.total, 0, "they start with nothing");

    share(&s, Role::Editor).await;

    let after = lists::for_user(&s.ctx, &s.theirs, all(), order(list::Field::Id))
        .await
        .unwrap();
    assert_eq!(after.total, 1, "the shared list is not on their screen");
    assert_eq!(after.items[0].id, s.list.id);
}

/// An editor may do everything to what is *on* the list, including removing items —
/// a decision, not an accident.
#[rstest]
#[tokio::test]
async fn an_editor_may_change_what_is_on_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    share(&s, Role::Editor).await;

    let added = items::quick_add(&s.ctx, &s.theirs, s.list.id, "2 kg apples")
        .await
        .unwrap();
    items::set_done(&s.ctx, &s.theirs, added.id, true)
        .await
        .unwrap();
    items::update(
        &s.ctx,
        &s.theirs,
        added.id,
        item::Name("pears".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();
    items::delete(&s.ctx, &s.theirs, added.id).await.unwrap();
    items::clear_done(&s.ctx, &s.theirs, s.list.id)
        .await
        .unwrap();
}

/// ...but not to the list itself.
#[rstest]
#[tokio::test]
async fn an_editor_may_not_rename_or_delete_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    share(&s, Role::Editor).await;

    assert_eq!(
        lists::update(&s.ctx, &s.theirs, s.list.id, list::Name("theirs".into()))
            .await
            .err(),
        Some(ServiceError::Forbidden),
        "an editor renamed a list"
    );
    assert_eq!(
        lists::delete(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::Forbidden)
    );
    assert_eq!(
        lists::invite(&s.ctx, &s.theirs, s.list.id, Role::Viewer)
            .await
            .err(),
        Some(ServiceError::Forbidden),
        "an editor invited someone"
    );
}

/// A viewer reads and nothing else — and gets `Forbidden`, not `NotFound`, because
/// they can already see the list. Pretending otherwise would read as a bug.
#[rstest]
#[tokio::test]
async fn a_viewer_may_only_read(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    share(&s, Role::Viewer).await;

    assert!(lists::get(&s.ctx, &s.theirs, s.list.id).await.is_ok());
    assert!(
        items::for_list(&s.ctx, &s.theirs, s.list.id, all(), order(item::Field::Id))
            .await
            .is_ok()
    );
    assert!(
        items::suggestions(&s.ctx, &s.theirs, s.list.id, 50, None)
            .await
            .is_ok()
    );

    for refusal in [
        items::quick_add(&s.ctx, &s.theirs, s.list.id, "smuggled")
            .await
            .err(),
        items::set_done(&s.ctx, &s.theirs, s.item.id, true)
            .await
            .err(),
        items::delete(&s.ctx, &s.theirs, s.item.id).await.err(),
        items::clear_done(&s.ctx, &s.theirs, s.list.id).await.err(),
        items::forget(&s.ctx, &s.theirs, s.list.id, item::Name("apples".into()))
            .await
            .err(),
    ] {
        assert_eq!(
            refusal,
            Some(ServiceError::Forbidden),
            "a viewer changed something"
        );
    }
}

/// The distinction the roles bought: a stranger is told nothing, a member is told no.
#[rstest]
#[tokio::test]
async fn a_stranger_is_told_nothing_and_a_viewer_is_told_no(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    assert_eq!(
        lists::get(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::NotFound),
        "a guessed id confirmed the list exists"
    );

    share(&s, Role::Viewer).await;

    assert_eq!(
        lists::delete(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::Forbidden),
        "someone who can see the list was told it does not exist"
    );
}

/// The memory is the list's, so it is shared with the list.
#[rstest]
#[tokio::test]
async fn history_is_shared_with_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    share(&s, Role::Editor).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, "Sourdough")
        .await
        .unwrap();

    let theirs = items::suggestions(&s.ctx, &s.theirs, s.list.id, 50, None)
        .await
        .unwrap();

    assert!(
        theirs.iter().any(|n| n.0 == "Sourdough"),
        "what one person added is not offered to the other: {theirs:?}"
    );
}

/// An invitation is single-role and not a way to become the owner.
#[rstest]
#[tokio::test]
async fn ownership_cannot_be_invited(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    assert_eq!(
        lists::invite(&s.ctx, &s.mine, s.list.id, Role::Owner)
            .await
            .err(),
        Some(ServiceError::InvalidInput)
    );
}

/// Following the same link twice is a double-click, and a stale lesser invitation
/// must not demote anybody.
#[rstest]
#[tokio::test]
async fn redeeming_twice_is_harmless_and_never_demotes(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let editor = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Editor)
        .await
        .unwrap();
    let viewer = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Viewer)
        .await
        .unwrap();

    lists::join(&s.ctx, &s.theirs, &editor).await.unwrap();
    lists::join(&s.ctx, &s.theirs, &editor).await.unwrap();
    lists::join(&s.ctx, &s.theirs, &viewer).await.unwrap();

    // still an editor
    assert!(
        items::quick_add(&s.ctx, &s.theirs, s.list.id, "still allowed")
            .await
            .is_ok(),
        "a stale viewer invitation demoted an editor"
    );
    let members = lists::members(&s.ctx, &s.mine, s.list.id).await.unwrap();
    assert_eq!(
        members.len(),
        1,
        "one person joined twice and became two members"
    );
}

#[rstest]
#[tokio::test]
async fn an_unknown_or_revoked_invitation_is_a_miss(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let token = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Editor)
        .await
        .unwrap();

    lists::revoke_invites(&s.ctx, &s.mine, s.list.id)
        .await
        .unwrap();

    assert_eq!(
        lists::join(&s.ctx, &s.theirs, &token).await.err(),
        Some(ServiceError::NotFound),
        "a revoked link still worked"
    );
    assert_eq!(
        lists::join(
            &s.ctx,
            &s.theirs,
            &crate::models::invite::Token("guessed".into())
        )
        .await
        .err(),
        Some(ServiceError::NotFound)
    );
}

/// The owner may remove anybody; anybody may remove themselves; the owner cannot
/// leave, because there is no transfer and a list without its owner could not be
/// renamed or deleted by anyone.
#[rstest]
#[tokio::test]
async fn leaving_and_removing(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    share(&s, Role::Editor).await;
    let them = s.theirs.person().unwrap().id;
    let me = s.mine.person().unwrap().id;

    // an editor cannot remove somebody else
    assert_eq!(
        lists::remove_member(&s.ctx, &s.theirs, s.list.id, me)
            .await
            .err(),
        Some(ServiceError::InvalidInput),
        "the owner was removable"
    );

    // but may leave
    lists::remove_member(&s.ctx, &s.theirs, s.list.id, them)
        .await
        .unwrap();
    assert_eq!(
        lists::get(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::NotFound),
        "leaving did not take the access away"
    );

    // and the owner may remove people
    share(&s, Role::Editor).await;
    lists::remove_member(&s.ctx, &s.mine, s.list.id, them)
        .await
        .unwrap();
    assert_eq!(
        lists::get(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::NotFound)
    );
}

/// The owner is not a membership row, so nothing can contradict who owns the list.
#[rstest]
#[tokio::test]
async fn the_owner_is_not_a_member_row(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    share(&s, Role::Editor).await;

    let members = lists::members(&s.ctx, &s.mine, s.list.id).await.unwrap();

    assert_eq!(members.len(), 1);
    assert_ne!(members[0].user_id, s.mine.person().unwrap().id);
}
