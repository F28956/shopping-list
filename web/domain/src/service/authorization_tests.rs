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

    let mine = items::suggestions(&s.ctx, &s.mine, 50).await.unwrap();

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

    let after = items::suggestions(&s.ctx, &s.mine, 50).await.unwrap();
    assert!(
        after.iter().any(|n| n.0 == "Sourdough"),
        "clearing the list forgot what was on it: {after:?}"
    );
}

/// Deleting the whole list, likewise.
#[rstest]
#[tokio::test]
async fn history_survives_deleting_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, "Rye")
        .await
        .unwrap();

    lists::delete(&s.ctx, &s.mine, s.list.id).await.unwrap();

    let after = items::suggestions(&s.ctx, &s.mine, 50).await.unwrap();
    assert!(after.iter().any(|n| n.0 == "Rye"), "{after:?}");
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

    let suggestions = items::suggestions(&s.ctx, &s.mine, 50).await.unwrap();
    let milks: Vec<_> = suggestions
        .iter()
        .filter(|n| n.0.to_lowercase() == "milk")
        .collect();
    assert_eq!(
        milks.len(),
        1,
        "one item became several memories: {suggestions:?}"
    );
    // shown back the way it was last written
    assert_eq!(milks[0].0, "milk");
}

/// A typo can be taken back.
#[rstest]
#[tokio::test]
async fn a_mistake_can_be_forgotten(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, "Mlik")
        .await
        .unwrap();

    items::forget(&s.ctx, &s.mine, item::Name("mlik".into()))
        .await
        .unwrap();

    let after = items::suggestions(&s.ctx, &s.mine, 50).await.unwrap();
    assert!(!after.iter().any(|n| n.0 == "Mlik"), "{after:?}");
    // and forgetting something that was never there is a miss, not a silent no-op
    assert_eq!(
        items::forget(&s.ctx, &s.mine, item::Name("never".into())).await,
        Err(ServiceError::NotFound)
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

    let held = Entry::for_user(&s.ctx.db, s.mine.person().unwrap().id, 10_000)
        .await
        .unwrap();
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

    let mine = items::suggestions(&s.ctx, &s.mine, 50).await.unwrap();

    assert!(!mine.iter().any(|n| n.0 == "Absinthe"), "{mine:?}");
    assert_eq!(
        items::forget(&s.ctx, &s.mine, item::Name("absinthe".into())).await,
        Err(ServiceError::NotFound),
        "one person forgot another person's history"
    );
}
