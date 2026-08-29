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
        None, item::Name("Apples".into()),
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
            None, None)
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
            None, item::Name("smuggled".into()),
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
        None, item::Name("Absinthe".into()),
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
        items::clear_done(&s.ctx, &s.theirs, s.list.id, None).await.err(),
        Some(ServiceError::NotFound),
        "a stranger cleared someone else's list"
    );
    assert!(
        items::get(&s.ctx, &s.mine, s.item.id).await.is_ok(),
        "the item was deleted anyway"
    );

    assert_eq!(
        items::clear_done(&s.ctx, &s.mine, s.list.id, None).await.unwrap(),
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
        None, item::Name("Bananas".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();
    items::set_done(&s.ctx, &s.mine, s.item.id, true)
        .await
        .unwrap();

    let gone = items::clear_done(&s.ctx, &s.mine, s.list.id, None).await.unwrap();

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
        items::clear_done(&s.ctx, &s.mine, s.list.id, None).await.unwrap(),
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
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "Sourdough")
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

    items::clear_done(&s.ctx, &s.mine, s.list.id, None).await.unwrap();

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
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "Rye")
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
    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "4 pint milk")
        .await
        .unwrap();
    assert_eq!(first.unit_id, Some(pint.id));
    tags::attach(&s.ctx, &s.mine, first.id, dairy.id)
        .await
        .unwrap();

    // second time: just the word
    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "milk")
        .await
        .unwrap();

    assert_eq!(again.unit_id, Some(pint.id), "the unit was not remembered");

    // Remembering the unit is what makes the second one the same thing as the first,
    // so it lands on the row already there -- and changes nothing about it.
    assert_eq!(again.id, first.id, "a second row was made for the same thing");
    assert_eq!(
        again.amount,
        item::Amount(4.0),
        "adding it again moved the amount"
    );

    let on_it = tags::for_item(&s.ctx, &s.mine, again.id).await.unwrap();
    assert_eq!(
        on_it.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![dairy.id],
        "the category was not remembered"
    );
}

/// Every tag comes back, not the last one attached.
///
/// The memory had a column for one tag, so filing something under a shop and a
/// category kept whichever was attached second. Clearing the list and adding it again
/// brought back half of it, which reads as the memory forgetting at random.
#[rstest]
#[tokio::test]
async fn every_remembered_tag_comes_back(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let produce = tags::create(&s.ctx, &Actor::System, tag::Name("produce".into()), None, None)
        .await
        .unwrap();
    let aldi = tags::create(&s.ctx, &Actor::System, tag::Name("aldi".into()), None, None)
        .await
        .unwrap();

    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "potatoes")
        .await
        .unwrap();
    for tag in [produce.id, aldi.id] {
        tags::attach(&s.ctx, &s.mine, first.id, tag).await.unwrap();
    }

    // Crossed off and cleared, the way a shop ends.
    items::set_done(&s.ctx, &s.mine, first.id, true).await.unwrap();
    items::clear_done(&s.ctx, &s.mine, s.list.id, None).await.unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "potatoes")
        .await
        .unwrap();

    let mut filed: Vec<_> = tags::for_item(&s.ctx, &s.mine, again.id)
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    filed.sort();
    let mut both = vec![produce.id, aldi.id];
    both.sort();

    assert_eq!(filed, both, "only some of the filing came back");
}

/// Taking a tag off is remembered too, and only that one.
#[rstest]
#[tokio::test]
async fn unfiling_forgets_one_tag_and_keeps_the_rest(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let produce = tags::create(&s.ctx, &Actor::System, tag::Name("produce".into()), None, None)
        .await
        .unwrap();
    let aldi = tags::create(&s.ctx, &Actor::System, tag::Name("aldi".into()), None, None)
        .await
        .unwrap();

    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "potatoes")
        .await
        .unwrap();
    for tag in [produce.id, aldi.id] {
        tags::attach(&s.ctx, &s.mine, first.id, tag).await.unwrap();
    }
    tags::detach(&s.ctx, &s.mine, first.id, aldi.id).await.unwrap();

    items::set_done(&s.ctx, &s.mine, first.id, true).await.unwrap();
    items::clear_done(&s.ctx, &s.mine, s.list.id, None).await.unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "potatoes")
        .await
        .unwrap();

    let filed: Vec<_> = tags::for_item(&s.ctx, &s.mine, again.id)
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(filed, vec![produce.id], "unfiling was not remembered");
}

/// Forgetting an item forgets what it was filed under, without a second delete.
#[rstest]
#[tokio::test]
async fn forgetting_an_item_forgets_its_filing(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let produce = tags::create(&s.ctx, &Actor::System, tag::Name("produce".into()), None, None)
        .await
        .unwrap();

    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "potatoes")
        .await
        .unwrap();
    tags::attach(&s.ctx, &s.mine, first.id, produce.id)
        .await
        .unwrap();

    items::forget(&s.ctx, &s.mine, s.list.id, item::Name("potatoes".into()))
        .await
        .unwrap();

    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM item_history_tags")
        .fetch_one(&s.ctx.db)
        .await
        .unwrap();
    assert_eq!(left, 0, "the filing outlived the memory it hung on");
}

/// The tag with this name, which the fixtures seed.
async fn tag_named(s: &Scene, name: &str) -> tag::Id {
    tags::get(&s.ctx, &s.mine, tag::Lookup::Name(tag::Name(name.into())))
        .await
        .unwrap_or_else(|_| panic!("the fixture has no tag called {name}"))
        .id
}

/// A list nobody has configured walks the shop the way it always did.
/// Editing an item back to no unit gives it `unit`, the same as adding one does.
///
/// The rule used to live in `create` alone, so an item added measured and then edited
/// to nothing kept the NULL -- and became exactly the near-duplicate the rule exists to
/// prevent, since "milk" and "1 unit milk" are then different units and different rows.
#[rstest]
#[tokio::test]
async fn editing_away_a_unit_gives_the_unit_unit(
    #[with(crate::models::fixtures::UNITS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;
    let kg = tag_free_unit(&s, "kg").await;

    let measured = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None,
        item::Name("Flour".into()),
        item::Amount(2.0),
        Some(kg),
    )
    .await
    .unwrap();
    assert_eq!(measured.unit_id, Some(kg));

    let plain = items::update(
        &s.ctx,
        &s.mine,
        measured.id,
        item::Name("Flour".into()),
        item::Amount(1.0),
        None,
        None,
    )
    .await
    .unwrap();

    let counted = tag_free_unit(&s, "unit").await;
    assert_eq!(plain.unit_id, Some(counted), "edited back to no unit at all");
}

/// A unit by name, for tests that need one the fixtures seeded.
async fn tag_free_unit(s: &Scene, name: &str) -> crate::models::unit::Id {
    crate::models::unit::Unit::get(
        &s.ctx.db,
        crate::models::unit::Lookup::Name(crate::models::unit::Name(name.into())),
    )
    .await
    .unwrap()
    .id
}

/// A rename made against a row somebody else has edited becomes a second row.
///
/// Anna sets the amount to 5, Ben renames it from a copy that still said 1. Both edits
/// survive: Anna's row keeps her number, and Ben's name arrives beside it carrying what
/// he was looking at. See scenario 5 of docs/offline.md.
///
/// Both edits are made by the same actor here, because who made them is not what the
/// rule turns on -- it turns on the row having moved since the copy was taken, which is
/// as true of two of somebody's own devices as of two people.
#[rstest]
#[tokio::test]
async fn a_rename_against_a_changed_row_splits_it(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let milk = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None,
        item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    // What Ben's phone had on screen when he typed the new name.
    let seen = items::Seen {
        name: item::Name("Milk".into()),
        amount: item::Amount(1.0),
        unit_id: milk.unit_id,
    };

    // Anna, meanwhile.
    items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("Milk".into()),
        item::Amount(5.0),
        milk.unit_id,
        None,
    )
    .await
    .unwrap();

    let renamed = items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("Whole milk".into()),
        item::Amount(1.0),
        milk.unit_id,
        Some(seen),
    )
    .await
    .unwrap();

    assert_ne!(renamed.id, milk.id, "the contested row was overwritten");
    assert_eq!(renamed.name, item::Name("Whole milk".into()));
    assert_eq!(
        renamed.amount,
        item::Amount(1.0),
        "the new row carries what the renaming device saw, not the other person's number"
    );

    let still = items::get(&s.ctx, &s.mine, milk.id).await.unwrap();
    assert_eq!(still.name, item::Name("Milk".into()));
    assert_eq!(still.amount, item::Amount(5.0), "the amount edit was lost");
}

/// A row nothing else has touched is not contested, however late the rename is.
#[rstest]
#[tokio::test]
async fn a_rename_against_an_unchanged_row_renames_it(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let milk = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None,
        item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    let renamed = items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("Whole milk".into()),
        item::Amount(1.0),
        milk.unit_id,
        Some(items::Seen {
            name: item::Name("Milk".into()),
            amount: item::Amount(1.0),
            unit_id: milk.unit_id,
        }),
    )
    .await
    .unwrap();

    assert_eq!(renamed.id, milk.id, "an uncontested rename must not split");
    assert_eq!(renamed.name, item::Name("Whole milk".into()));
}

/// Two people arguing about one number is one argument, and one of them wins.
///
/// Splitting here would leave two rows both called `Milk`, which is a worse answer
/// than either person losing their number.
#[rstest]
#[tokio::test]
async fn an_edit_that_is_not_a_rename_never_splits(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let milk = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None,
        item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("Milk".into()),
        item::Amount(5.0),
        milk.unit_id,
        None,
    )
    .await
    .unwrap();

    let after = items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("Milk".into()),
        item::Amount(9.0),
        milk.unit_id,
        Some(items::Seen {
            name: item::Name("Milk".into()),
            amount: item::Amount(1.0),
            unit_id: milk.unit_id,
        }),
    )
    .await
    .unwrap();

    assert_eq!(after.id, milk.id);
    assert_eq!(after.amount, item::Amount(9.0), "latest wins, in place");
}

/// Capitalisation is the model's, not a rename.
///
/// `Item::update` trims and capitalises what it stores, so a device that sends back
/// `milk` for a row the server calls `Milk` is not renaming anything -- and splitting a
/// row in two over a capital letter would be an unpleasant surprise.
#[rstest]
#[tokio::test]
async fn a_difference_the_model_would_have_made_is_not_a_rename(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let milk = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None,
        item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("Milk".into()),
        item::Amount(5.0),
        milk.unit_id,
        None,
    )
    .await
    .unwrap();

    let after = items::update(
        &s.ctx,
        &s.mine,
        milk.id,
        item::Name("  milk ".into()),
        item::Amount(2.0),
        milk.unit_id,
        Some(items::Seen {
            name: item::Name("Milk".into()),
            amount: item::Amount(1.0),
            unit_id: milk.unit_id,
        }),
    )
    .await
    .unwrap();

    assert_eq!(after.id, milk.id, "a capital letter split the row in two");
}

/// A sweep that names the rows it meant takes those and leaves the rest.
///
/// The case: somebody taps "clear 2 done" in a shop with no signal, somebody at home
/// ticks another thing off, and the queue reaches the server an hour later. Replayed as
/// "everything that is done", it would take the third as well -- which nobody asked for
/// and nobody would connect to a button pressed an hour ago in another building.
#[rstest]
#[tokio::test]
async fn a_sweep_clears_the_rows_it_meant_and_no_others(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    async fn add(s: &Scene, name: &str) -> item::Item {
        items::create(
            &s.ctx,
            &s.mine,
            s.list.id,
            None,
            item::Name(name.into()),
            item::Amount(1.0),
            None,
        )
        .await
        .unwrap()
    }

    let bread = add(&s, "Bread").await;
    let milk = add(&s, "Milk").await;
    let eggs = add(&s, "Eggs").await;

    for one in [&bread, &milk, &eggs] {
        items::set_done(&s.ctx, &s.mine, one.id, true).await.unwrap();
    }

    // What the shop meant: the two that were done when the button was pressed.
    let cleared = items::clear_done(&s.ctx, &s.mine, s.list.id, Some(&[bread.id, milk.id]))
        .await
        .unwrap();

    assert_eq!(cleared, 2);
    let left: Vec<_> = items::for_list(&s.ctx, &s.mine, s.list.id, all(), order(item::Field::Id))
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert!(left.contains(&eggs.id), "somebody else's tick was swept away");
    assert!(!left.contains(&bread.id));
    assert!(!left.contains(&milk.id));
}

/// A named row that is no longer done is one somebody put back, and putting something
/// back is a newer decision than a sweep queued before it.
#[rstest]
#[tokio::test]
async fn a_sweep_leaves_what_was_put_back(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let milk = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None,
        item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    items::set_done(&s.ctx, &s.mine, milk.id, true).await.unwrap();
    // ... and then somebody needed it after all.
    items::set_done(&s.ctx, &s.mine, milk.id, false).await.unwrap();

    let cleared = items::clear_done(&s.ctx, &s.mine, s.list.id, Some(&[milk.id]))
        .await
        .unwrap();

    assert_eq!(cleared, 0, "a sweep must not delete something outstanding");
}

/// Rows already gone are not an error: somebody deleting one first is the same outcome
/// by another route, and a replayed sweep must not fail because it got its way early.
#[rstest]
#[tokio::test]
async fn a_sweep_shrugs_at_rows_that_have_gone(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let cleared = items::clear_done(&s.ctx, &s.mine, s.list.id, Some(&[item::Id(9999)]))
        .await
        .unwrap();

    assert_eq!(cleared, 0);
}

/// Joining tells everybody it concerns, not just the person who joined.
///
/// Three screens go out of date the moment somebody accepts an invitation: their own
/// list of lists, the owner's (which counts who a list is shared with), and the list
/// itself (whose share sheet says who can see it). Announcing only the first is what
/// left that sheet reading "who can see it: you" while somebody else was already
/// looking at the list.
#[rstest]
#[tokio::test]
async fn joining_is_announced_to_the_owner_and_to_the_list(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    // Subscribed before the join: these channels carry only what is sent afterwards.
    let mut lists_of_owner = s.ctx.changes.watch_lists();
    let mut the_list = s.ctx.changes.watch();

    let token = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Editor)
        .await
        .unwrap();
    lists::join(&s.ctx, &s.theirs, &token).await.unwrap();

    let owner = s.mine.person().unwrap().id;
    let mut told = Vec::new();
    while let Ok(heard) = lists_of_owner.try_recv() {
        told.push(heard.user_id);
    }
    assert!(told.contains(&owner), "the owner was not told: {told:?}");

    assert_eq!(
        the_list.try_recv().map(|heard| heard.list_id).ok(),
        Some(s.list.id),
        "the list itself was not told"
    );
}

#[rstest]
#[tokio::test]
async fn an_order_is_inherited_from_whoever_set_one_first(
    #[with(crate::models::fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;
    let urgent = tag_named(&s, "urgent").await;

    // They are given the list to read, and set nothing.
    let token = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Viewer)
        .await
        .unwrap();
    lists::join(&s.ctx, &s.theirs, &token).await.unwrap();

    tags::set_order(&s.ctx, &s.mine, s.list.id, &[urgent])
        .await
        .unwrap();

    let theirs = tags::order_for(&s.ctx, &s.theirs, s.list.id).await.unwrap();
    assert_eq!(theirs.first().map(|t| t.id), Some(urgent), "not inherited");

    // Their own choice outranks what they inherited...
    let aldi = tag_named(&s, "aldi").await;
    tags::set_order(&s.ctx, &s.theirs, s.list.id, &[aldi])
        .await
        .unwrap();
    let theirs = tags::order_for(&s.ctx, &s.theirs, s.list.id).await.unwrap();
    assert_eq!(theirs.first().map(|t| t.id), Some(aldi));

    // ... and does not disturb the person they inherited it from.
    let mine = tags::order_for(&s.ctx, &s.mine, s.list.id).await.unwrap();
    assert_eq!(mine.first().map(|t| t.id), Some(urgent), "mine was changed");

    // Clearing theirs puts them back on what they inherit.
    tags::set_order(&s.ctx, &s.theirs, s.list.id, &[]).await.unwrap();
    let theirs = tags::order_for(&s.ctx, &s.theirs, s.list.id).await.unwrap();
    assert_eq!(theirs.first().map(|t| t.id), Some(urgent));
}

/// A viewer decides the order they read a list in. It changes their screen and
/// nothing about the list, and permission to read is not permission to be sorted.
#[rstest]
#[tokio::test]
async fn a_viewer_may_order_their_own_view(
    #[with(crate::models::fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;
    let aldi = tag_named(&s, "aldi").await;

    let token = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Viewer)
        .await
        .unwrap();
    lists::join(&s.ctx, &s.theirs, &token).await.unwrap();

    assert!(tags::set_order(&s.ctx, &s.theirs, s.list.id, &[aldi]).await.is_ok());
}

/// A stranger cannot read the order, nor set one.
#[rstest]
#[tokio::test]
async fn a_strangers_order_is_refused(
    #[with(crate::models::fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;
    let aldi = tag_named(&s, "aldi").await;

    assert_eq!(
        tags::order_for(&s.ctx, &s.theirs, s.list.id).await.err(),
        Some(ServiceError::NotFound)
    );
    assert_eq!(
        tags::set_order(&s.ctx, &s.theirs, s.list.id, &[aldi]).await.err(),
        Some(ServiceError::NotFound)
    );
}

/// A tag that does not exist is the caller's mistake. Stored, it would be a position
/// the resolver silently drops, and nothing would say why the order looked wrong.
#[rstest]
#[tokio::test]
async fn an_unknown_tag_cannot_be_placed(
    #[with(crate::models::fixtures::TAGS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;

    assert_eq!(
        tags::set_order(&s.ctx, &s.mine, s.list.id, &[tag::Id(9999)])
            .await
            .err(),
        Some(ServiceError::NotFound)
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
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "4 pint milk")
        .await
        .unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "2 litre milk")
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
    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "milk")
        .await
        .unwrap();
    tags::attach(&s.ctx, &s.mine, first.id, dairy.id)
        .await
        .unwrap();
    tags::detach(&s.ctx, &s.mine, first.id, dairy.id)
        .await
        .unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "milk")
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

    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "Milk")
        .await
        .unwrap();
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "MILK")
        .await
        .unwrap();
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "  milk ")
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

/// Suggestions are capped in the service, not by whoever is asking.
///
/// The browser used to show every match and the phone the first six, which is two
/// answers to one question. It is one now, so this is where it is checked.
#[rstest]
#[tokio::test]
async fn suggestions_are_capped(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    // Lettered, not numbered: a trailing number is read as a quantity, so
    // "apple sort 3" would be three of "apple sort" and ten names would collapse
    // into one memory -- which is the parser working, and a useless fixture.
    for suffix in ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"] {
        items::quick_add(&s.ctx, &s.mine, s.list.id, None, &format!("apple sort {suffix}"))
            .await
            .unwrap();
    }

    let offered = items::suggestions(&s.ctx, &s.mine, s.list.id, 500, Some("apple"))
        .await
        .unwrap();
    assert_eq!(offered.len(), items::SUGGESTIONS);
}

/// What has already been typed in full is not a suggestion: accepting it would change
/// nothing, and it costs a row that a real one could have had.
#[rstest]
#[tokio::test]
async fn what_is_already_typed_is_not_offered(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "Milk").await.unwrap();
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "Milk chocolate")
        .await
        .unwrap();

    let offered = items::suggestions(&s.ctx, &s.mine, s.list.id, 500, Some("Milk"))
        .await
        .unwrap();

    assert_eq!(
        offered.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(),
        vec!["Milk chocolate"],
        "the exact match should have been dropped, the other kept"
    );

    // However it was capitalised: the comparison is the same one the matcher uses.
    let offered = items::suggestions(&s.ctx, &s.mine, s.list.id, 500, Some("  mILk "))
        .await
        .unwrap();
    assert!(!offered.iter().any(|n| n.0 == "Milk"));
}

/// Adding what the list already wants changes nothing.
///
/// It is already there, and that is the whole answer. Two rows saying `Milk` are
/// never two intentions, and neither is four kilograms when two people each asked for
/// two -- somebody adding a thing has not looked at the amount.
///
/// Being idempotent is also what makes an add safe to replay: the same event twice,
/// or an hour late, means the same thing both times.
#[rstest]
#[tokio::test]
async fn adding_the_same_thing_twice_changes_nothing(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let kg = units::create(&s.ctx, &Actor::System, unit::Name("kg".into()))
        .await
        .unwrap();
    // Its own list: the scene's already has apples on it, which is the thing under
    // test here.
    let list = lists::create(&s.ctx, &s.mine, list::Name("Empty".into()))
        .await
        .unwrap();

    let first = items::create(
        &s.ctx,
        &s.mine,
        list.id,
        None, item::Name("Apples".into()),
        item::Amount(2.0),
        Some(kg.id),
    )
    .await
    .unwrap();

    let again = items::create(
        &s.ctx,
        &s.mine,
        list.id,
        None,
        // However it is spelled: the comparison ignores case and surrounding space,
        // in Rust, because SQLite's lower() is ASCII-only.
        item::Name("  apples ".into()),
        item::Amount(1.0),
        Some(kg.id),
    )
    .await
    .unwrap();

    assert_eq!(again.id, first.id, "a second row was made");
    assert_eq!(again.amount, item::Amount(2.0), "the amount was moved under them");
    assert_eq!(again.name, item::Name("Apples".into()), "the spelling stood");

    let page = items::for_list(&s.ctx, &s.mine, list.id, all(), order(item::Field::Id))
        .await
        .unwrap();
    assert_eq!(page.total, 1);
}

/// Different units are not the same thing. Three of something and two kilograms of
/// it do not add up to five of anything.
#[rstest]
#[tokio::test]
async fn a_different_unit_is_a_different_row(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let kg = units::create(&s.ctx, &Actor::System, unit::Name("kg".into()))
        .await
        .unwrap();
    let list = lists::create(&s.ctx, &s.mine, list::Name("Empty".into()))
        .await
        .unwrap();

    for unit in [Some(kg.id), None] {
        items::create(
            &s.ctx,
            &s.mine,
            list.id,
            None, item::Name("Apples".into()),
            item::Amount(2.0),
            unit,
        )
        .await
        .unwrap();
    }

    let page = items::for_list(&s.ctx, &s.mine, list.id, all(), order(item::Field::Id))
        .await
        .unwrap();
    assert_eq!(page.total, 2, "they were folded together: {page:?}");
}

/// Adding something already crossed off puts it back. That is how you say you need
/// it after all, and it is the commonest reason to type a name that is already there.
#[rstest]
#[tokio::test]
async fn adding_something_crossed_off_puts_it_back(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;

    let first = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None, item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();
    items::set_done(&s.ctx, &s.mine, first.id, true).await.unwrap();

    let again = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None, item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    assert_eq!(again.id, first.id);
    assert!(again.done_at.is_none(), "it stayed crossed off");
    assert_eq!(
        again.amount,
        item::Amount(1.0),
        "putting it back changed the amount"
    );
}

/// An outstanding row wins over a crossed-off one: adding milk when milk is on the
/// list means the one you still need, not the one already in the trolley.
#[rstest]
#[tokio::test]
async fn an_outstanding_row_is_preferred(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let list = lists::create(&s.ctx, &s.mine, list::Name("Empty".into()))
        .await
        .unwrap();

    let done = items::create(
        &s.ctx,
        &s.mine,
        list.id,
        None, item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();
    items::set_done(&s.ctx, &s.mine, done.id, true).await.unwrap();

    // A second row, outstanding, made while the first was crossed off.
    let outstanding = crate::models::item::Item::create(
        &s.ctx.db,
        item::Uuid::mint(),
        list.id,
        item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    let again = items::create(
        &s.ctx,
        &s.mine,
        list.id,
        None, item::Name("Milk".into()),
        item::Amount(1.0),
        None,
    )
    .await
    .unwrap();

    assert_eq!(again.id, outstanding.id, "it went to the crossed-off one");

    // Re-read: `done` is the row as it was returned before it was ticked off, and a
    // stale copy would assert nothing about what is stored.
    let still_done = items::get(&s.ctx, &s.mine, done.id).await.unwrap();
    assert!(still_done.done_at.is_some(), "the crossed-off row was disturbed");
    assert_eq!(still_done.amount, item::Amount(1.0));
}

/// The merge is an addition, and `CHECK (amount > 0)` only guards the insert: `2 + -1`
/// is 1, which the column is perfectly happy with. Without a check of its own, adding
/// a negative amount of something took some of it away.
#[rstest]
#[case::zero(0.0)]
#[case::negative(-1.0)]
#[case::not_a_number(f64::NAN)]
#[tokio::test]
async fn a_non_positive_amount_cannot_shrink_an_item(
    #[future(awt)] pool: SqlitePool,
    #[case] amount: f64,
) {
    let s = scene(pool).await;
    let first = items::create(
        &s.ctx,
        &s.mine,
        s.list.id,
        None, item::Name("Apples".into()),
        item::Amount(2.0),
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        items::create(
            &s.ctx,
            &s.mine,
            s.list.id,
            None, item::Name("Apples".into()),
            item::Amount(amount),
            None,
        )
        .await
        .err(),
        Some(ServiceError::InvalidInput)
    );

    // Against what the row actually held, not a number written here: the scene this
    // runs in already has apples on it, so `first` merged too.
    let unchanged = items::get(&s.ctx, &s.mine, first.id).await.unwrap();
    assert_eq!(unchanged.amount, first.amount, "the item was changed");
}

/// Counted rather than measured is still a unit.
///
/// Left as NULL, "milk" and "1 unit milk" are different units and so different rows,
/// and a list grows a near-duplicate that nothing will ever merge.
#[rstest]
#[tokio::test]
async fn an_item_with_no_unit_gets_the_unit_unit(
    #[with(crate::models::fixtures::UNITS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;
    let list = lists::create(&s.ctx, &s.mine, list::Name("Empty".into()))
        .await
        .unwrap();

    let plain = items::quick_add(&s.ctx, &s.mine, list.id, None, "milk").await.unwrap();
    let spelled = items::quick_add(&s.ctx, &s.mine, list.id, None, "1 unit milk")
        .await
        .unwrap();

    assert!(plain.unit_id.is_some(), "no unit was filled in");
    assert_eq!(spelled.id, plain.id, "the two were not recognised as one thing");
}

/// A typo can be taken back.
/// A context over a server that admits exactly these addresses and nobody else.
///
/// The fixture leaves a server claimed and open, because a test about lists should not
/// have to admit anybody first. These tests are the ones actually about admission, so
/// they say what they mean: closed, and holding this list.
///
/// Called a second time it replaces the list, which is how "the owner withdrew that
/// address" is written — including withdrawing one somebody has already used, since
/// the binding row goes with it.
async fn admitting(pool: &SqlitePool, addresses: &str) -> Ctx {
    use crate::models::admission::Admitted;
    use crate::models::user::Email;

    let ctx = Ctx::new(pool.clone());
    sqlx::raw_sql("UPDATE server SET admits_anyone = 0; DELETE FROM admitted_emails;")
        .execute(&ctx.db)
        .await
        .unwrap();

    for address in addresses.split(',').map(str::trim).filter(|a| !a.is_empty()) {
        Admitted::seed(&ctx.db, &Email(address.to_string()), None)
            .await
            .unwrap();
    }

    ctx
}

/// The same person, arriving the other way, is the same person.
///
/// Android signs in with Google and the Apple clients sign in with Apple, so one human
/// has two subjects. Matching on the address is what keeps them one account with one
/// list rather than two accounts with none.
#[rstest]
#[tokio::test]
async fn two_providers_one_person(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::identity;

    let ctx = admitting(&pool, "me@example.com").await;

    let first = identity::from_claims(
        &ctx,
        "google",
        user::Sub("google|me".into()),
        Some(user::Name("Me".into())),
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();

    let second = identity::from_claims(
        &ctx,
        "apple",
        user::Sub("apple|me".into()),
        // Apple never sends a name in the token.
        None,
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();

    assert_eq!(
        first.person().unwrap().id,
        second.person().unwrap().id,
        "the second sign-in made a second person"
    );
    assert_eq!(
        second.person().unwrap().name,
        Some(user::Name("Me".into())),
        "the name from the first provider was cleared by a token that carried none"
    );

    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 1);
}

/// Apple sends an address on the first authorisation and never again.
///
/// Admission reads the address, so a check that only ever looked at the token would let
/// somebody in once and refuse them for ever after — which is the shape of bug that
/// looks like a flaky login.
#[rstest]
#[tokio::test]
async fn a_later_sign_in_with_no_address_is_still_admitted(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::identity;

    let ctx = admitting(&pool, "me@example.com").await;

    identity::from_claims(
        &ctx,
        "apple",
        user::Sub("apple|me".into()),
        None,
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();

    // Every sign-in after the first, as Apple actually sends them.
    let again = identity::from_claims(&ctx, "apple", user::Sub("apple|me".into()), None, None).await;

    assert!(again.is_ok(), "a returning person was refused: {again:?}");
    assert_eq!(
        again.unwrap().person().unwrap().email,
        Some(user::Email("me@example.com".into())),
        "the stored address was lost"
    );
}

/// Somebody taken off the list stops getting in, even though their token says nothing
/// about an address any more.
#[rstest]
#[tokio::test]
async fn removing_an_address_stops_a_nameless_token_too(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::identity;

    let welcome = admitting(&pool, "me@example.com").await;
    identity::from_claims(
        &welcome,
        "apple",
        user::Sub("apple|me".into()),
        None,
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();

    let removed = admitting(&pool, "someone@else.com").await;
    let refused =
        identity::from_claims(&removed, "apple", user::Sub("apple|me".into()), None, None).await;

    assert_eq!(refused.err(), Some(ServiceError::NotAdmitted));
}

/// Two providers may issue the same subject string, and it would mean two people.
#[rstest]
#[tokio::test]
async fn the_same_subject_from_two_providers_is_two_people(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::identity;

    let ctx = Ctx::new(pool.clone());

    let one = identity::from_claims(
        &ctx,
        "google",
        user::Sub("000123".into()),
        None,
        Some(user::Email("one@example.com".into())),
    )
    .await
    .unwrap();

    let other = identity::from_claims(
        &ctx,
        "apple",
        user::Sub("000123".into()),
        None,
        Some(user::Email("other@example.com".into())),
    )
    .await
    .unwrap();

    assert_ne!(one.person().unwrap().id, other.person().unwrap().id);
}

/// A stranger is refused before a row is written for them.
///
/// The `find_or_create` on the far side of this call is what makes an account, so
/// checking after it would leave one behind for everybody who tried the door.
#[rstest]
#[tokio::test]
async fn an_unlisted_address_cannot_sign_in(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::identity;

    let ctx = admitting(&pool, "me@example.com").await;

    assert_eq!(
        identity::from_claims(
            &ctx,
            "google",
            user::Sub("google-oauth2|stranger".into()),
            Some(user::Name("Stranger".into())),
            Some(user::Email("stranger@example.com".into())),
        )
        .await
        .err(),
        // Not `Forbidden`, which is a sentence about a list. This is a sentence about
        // the account, and the two reach a person as different words.
        Some(ServiceError::NotAdmitted)
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
            "google",
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
    use crate::service::identity;

    let welcome = admitting(&pool, "me@example.com").await;
    let actor = identity::from_claims(
        &welcome,
        "google",
        user::Sub("google-oauth2|me".into()),
        Some(user::Name("Me".into())),
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();
    let id = actor.person().unwrap().id.0;

    assert!(identity::from_session(&welcome, id).await.unwrap().is_some());

    let removed = admitting(&pool, "someone-else@example.com").await;
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
        items::quick_add(&s.ctx, &s.mine, s.list.id, None, &format!("item-{n}"))
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
    items::quick_add(&s.ctx, &s.theirs, theirs_list.id, None, "Absinthe")
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

    let added = items::quick_add(&s.ctx, &s.theirs, s.list.id, None, "2 kg apples")
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
        None,
    )
    .await
    .unwrap();
    items::delete(&s.ctx, &s.theirs, added.id).await.unwrap();
    items::clear_done(&s.ctx, &s.theirs, s.list.id, None)
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
        items::quick_add(&s.ctx, &s.theirs, s.list.id, None, "smuggled")
            .await
            .err(),
        items::set_done(&s.ctx, &s.theirs, s.item.id, true)
            .await
            .err(),
        items::delete(&s.ctx, &s.theirs, s.item.id).await.err(),
        items::clear_done(&s.ctx, &s.theirs, s.list.id, None).await.err(),
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
    items::quick_add(&s.ctx, &s.mine, s.list.id, None, "Sourdough")
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

/// A spent link grants nothing, not even to somebody already on the list.
///
/// The narrow version of the same leak: a viewer who came by their own link and later
/// got hold of a used editor one would be promoted by it.
#[rstest]
#[tokio::test]
async fn a_used_link_cannot_promote_a_member(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let third = person(&s.ctx.db, "google-oauth2|third").await;

    // They arrive as a viewer, by a link of their own.
    let viewing = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Viewer)
        .await
        .unwrap();
    lists::join(&s.ctx, &s.theirs, &viewing).await.unwrap();

    // Somebody else is invited as an editor and uses their link.
    let editing = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Editor)
        .await
        .unwrap();
    lists::join(&s.ctx, &third, &editing).await.unwrap();

    // The viewer then gets hold of that spent editor link.
    lists::join(&s.ctx, &s.theirs, &editing).await.unwrap();

    assert_eq!(
        lists::role(&s.ctx, &s.theirs, s.list.id).await.unwrap(),
        Role::Viewer,
        "a spent link promoted somebody"
    );
    assert_eq!(
        items::quick_add(&s.ctx, &s.theirs, s.list.id, None, "not allowed")
            .await
            .err(),
        Some(ServiceError::Forbidden)
    );
}

/// A spent link is spent for everybody else.
///
/// A link lives a week. Without this, a forwarded message or a screenshot let somebody
/// join days after the person it was written for already had — and the owner, who
/// never sees the link again, has no way to tell one outstanding link from another.
#[rstest]
#[tokio::test]
async fn a_used_link_does_not_admit_anybody_else(#[future(awt)] pool: SqlitePool) {
    let s = scene(pool).await;
    let third = person(&s.ctx.db, "google-oauth2|third").await;

    let token = lists::invite(&s.ctx, &s.mine, s.list.id, Role::Editor)
        .await
        .unwrap();

    lists::join(&s.ctx, &s.theirs, &token).await.unwrap();

    assert_eq!(
        lists::join(&s.ctx, &third, &token).await.err(),
        Some(ServiceError::NotFound),
        "a spent link admitted a second person"
    );

    // ... and the person it was written for can still follow it again.
    assert!(lists::join(&s.ctx, &s.theirs, &token).await.is_ok());
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
        items::quick_add(&s.ctx, &s.theirs, s.list.id, None, "still allowed")
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

/// A7, and the single property that makes an admission list worth having.
///
/// The removal and the refusal go through **the same `Ctx`**. Rebuilding it would
/// prove only that a fresh read sees fresh rows, which is not the question — the
/// question is whether anything between them is holding the old answer. A cache added
/// later with a time-based expiry would quietly turn "you are out now" into "you are
/// out within five minutes", and this is the test that would notice.
#[rstest]
#[tokio::test]
async fn withdrawing_an_address_takes_effect_on_the_very_next_request(
    #[future(awt)] pool: SqlitePool,
) {
    use crate::models::admission::Admitted;
    use crate::models::user;
    use crate::service::identity;

    let ctx = admitting(&pool, "me@example.com").await;

    let actor = identity::from_claims(
        &ctx,
        "google",
        user::Sub("me".into()),
        None,
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();
    let id = actor.person().unwrap().id;

    assert!(identity::from_session(&ctx, id.0).await.unwrap().is_some());

    Admitted::remove(&ctx.db, &user::Email("me@example.com".into()))
        .await
        .unwrap();

    assert!(
        identity::from_session(&ctx, id.0).await.unwrap().is_none(),
        "the session outlived the permission"
    );
    assert_eq!(
        identity::from_claims(&ctx, "google", user::Sub("me".into()), None, None)
            .await
            .err(),
        Some(ServiceError::NotAdmitted),
        "signing in again outlived the permission"
    );
}

/// The seed is for a database with nothing in it, and must not undo somebody's work.
///
/// A variable left behind in a unit file is the likeliest way this goes wrong: an
/// owner withdraws an address through the app, the box reboots, and the address comes
/// back with no sign of why.
#[rstest]
#[tokio::test]
async fn seeding_does_nothing_to_a_server_that_has_been_set_up(#[future(awt)] pool: SqlitePool) {
    use crate::models::admission::Admitted;
    use crate::models::user::Email;
    use crate::service::admission::{self, Admission};

    let ctx = admitting(&pool, "her@example.com").await;

    admission::seed(&ctx, Some(&Admission::parse("stranger@example.com").unwrap()))
        .await
        .unwrap();

    assert!(
        !Admitted::admits_email(&ctx.db, &Email("stranger@example.com".into()))
            .await
            .unwrap(),
        "a stale variable let somebody back in"
    );
    assert!(
        Admitted::admits_email(&ctx.db, &Email("her@example.com".into()))
            .await
            .unwrap()
    );
}

/// A server that already has people cannot ask who arrived first — it would hand
/// itself to whoever opened the app next.
#[rstest]
#[tokio::test]
async fn seeding_an_existing_server_gives_it_to_the_earliest_person(
    #[future(awt)] pool: SqlitePool,
) {
    use crate::models::admission::{Server, owners};
    use crate::models::user::{self, User};
    use crate::service::admission::{self as admission_service, Admission};
    use crate::service::identity;

    let ctx = Ctx::new(pool.clone());

    // Two people, in a known order, on a server that was open before any of this
    // existed -- which is the shape of every server this migration will meet.
    for who in ["first", "second"] {
        identity::from_claims(
            &ctx,
            "google",
            user::Sub(who.into()),
            None,
            Some(user::Email(format!("{who}@example.com"))),
        )
        .await
        .unwrap();
    }

    sqlx::raw_sql("UPDATE server SET admits_anyone = 0, claimed_at = NULL")
        .execute(&ctx.db)
        .await
        .unwrap();

    admission_service::seed(&ctx, Some(&Admission::parse("first@example.com").unwrap()))
        .await
        .unwrap();

    let earliest = User::earliest(&ctx.db).await.unwrap().unwrap();
    assert_eq!(earliest.email, Some(user::Email("first@example.com".into())));
    assert_eq!(owners(&ctx.db).await.unwrap(), vec![earliest.id]);
    assert!(
        Server::is_claimed(&ctx.db).await.unwrap(),
        "an upgraded server was left unclaimed, so a stranger could still claim it"
    );
}

// ---------------------------------------------------------------------------
// Who may administer the server
// ---------------------------------------------------------------------------

/// An owner and somebody who merely uses the server, both admitted.
async fn a_server_with_an_owner(pool: &SqlitePool) -> (Ctx, Actor, Actor) {
    use crate::models::admission::set_owner;
    use crate::models::user;
    use crate::service::identity;

    let ctx = admitting(pool, "owner@example.com, guest@example.com").await;

    let mut who = Vec::new();
    for name in ["owner", "guest"] {
        who.push(
            identity::from_claims(
                &ctx,
                "google",
                user::Sub(name.into()),
                None,
                Some(user::Email(format!("{name}@example.com"))),
            )
            .await
            .unwrap(),
        );
    }

    let guest = who.pop().unwrap();
    let owner = who.pop().unwrap();
    set_owner(&ctx.db, owner.person().unwrap().id, true).await.unwrap();

    (ctx, owner, guest)
}

#[rstest]
#[tokio::test]
async fn an_owner_admits_and_withdraws(#[future(awt)] pool: SqlitePool) {
    use crate::models::user::Email;
    use crate::service::admission;

    let (ctx, owner, _) = a_server_with_an_owner(&pool).await;
    let her = Email("her@example.com".into());

    admission::admit(&ctx, &owner, &her, None).await.unwrap();
    assert!(admission::admits_email(&ctx, Some(&her)).await.unwrap());

    admission::withdraw(&ctx, &owner, &her).await.unwrap();
    assert!(!admission::admits_email(&ctx, Some(&her)).await.unwrap());
}

/// Everything on the owner's screen is refused to somebody who merely uses the server.
/// Using it is not administering it.
#[rstest]
#[tokio::test]
async fn a_guest_cannot_administer_anything(#[future(awt)] pool: SqlitePool) {
    use crate::models::user::Email;
    use crate::service::admission;

    let (ctx, owner, guest) = a_server_with_an_owner(&pool).await;
    let her = Email("her@example.com".into());
    let owner_id = owner.person().unwrap().id;

    assert_eq!(admission::listing(&ctx, &guest).await.err(), Some(ServiceError::Forbidden));
    assert_eq!(
        admission::admit(&ctx, &guest, &her, None).await.err(),
        Some(ServiceError::Forbidden)
    );
    assert_eq!(
        admission::withdraw(&ctx, &guest, &Email("owner@example.com".into()))
            .await
            .err(),
        Some(ServiceError::Forbidden)
    );
    assert_eq!(
        admission::set_ownership(&ctx, &guest, guest.person().unwrap().id, true)
            .await
            .err(),
        Some(ServiceError::Forbidden),
        "a guest promoted themselves"
    );
    assert_eq!(
        admission::set_ownership(&ctx, &guest, owner_id, false).await.err(),
        Some(ServiceError::Forbidden),
        "a guest demoted the owner"
    );
    assert_eq!(
        admission::set_open(&ctx, &guest, true).await.err(),
        Some(ServiceError::Forbidden),
        "a guest opened the server to everybody"
    );
}

/// A5. A server with no owner has no way back that does not involve `sqlite3` on the
/// host, and the person most likely to arrange it is the owner tidying up.
#[rstest]
#[tokio::test]
async fn the_last_owner_cannot_be_demoted(#[future(awt)] pool: SqlitePool) {
    use crate::service::admission;

    let (ctx, owner, _) = a_server_with_an_owner(&pool).await;
    let owner_id = owner.person().unwrap().id;

    assert_eq!(
        admission::set_ownership(&ctx, &owner, owner_id, false).await.err(),
        Some(ServiceError::InUse),
        "the last owner demoted themselves"
    );
    assert!(admission::is_owner(&ctx, owner_id).await.unwrap());
}

/// The same rule reached from the other direction: withdrawing your own admission
/// signs you out, and removal takes effect on the very next request.
#[rstest]
#[tokio::test]
async fn the_last_owner_cannot_withdraw_their_own_admission(#[future(awt)] pool: SqlitePool) {
    use crate::models::user::Email;
    use crate::service::admission;

    let (ctx, owner, _) = a_server_with_an_owner(&pool).await;

    assert_eq!(
        admission::withdraw(&ctx, &owner, &Email("owner@example.com".into()))
            .await
            .err(),
        Some(ServiceError::InUse)
    );
}

/// Once there are two, either may go — they are equal, which is the whole point of a
/// flag rather than a hierarchy.
#[rstest]
#[tokio::test]
async fn with_two_owners_either_may_step_down(#[future(awt)] pool: SqlitePool) {
    use crate::service::admission;

    let (ctx, owner, guest) = a_server_with_an_owner(&pool).await;
    let owner_id = owner.person().unwrap().id;
    let guest_id = guest.person().unwrap().id;

    admission::set_ownership(&ctx, &owner, guest_id, true).await.unwrap();

    // The promoted one demotes the one who promoted them, which is allowed on
    // purpose: an owner who cannot be demoted by somebody they promoted is a
    // hierarchy nobody asked for.
    admission::set_ownership(&ctx, &guest, owner_id, false).await.unwrap();

    assert!(!admission::is_owner(&ctx, owner_id).await.unwrap());
    assert!(admission::is_owner(&ctx, guest_id).await.unwrap());
}

/// An owner who cannot sign in is the same problem as no owner, reached from a
/// different direction.
#[rstest]
#[tokio::test]
async fn somebody_who_cannot_sign_in_cannot_be_made_an_owner(#[future(awt)] pool: SqlitePool) {
    use crate::models::user::Email;
    use crate::service::admission;

    let (ctx, owner, guest) = a_server_with_an_owner(&pool).await;
    let guest_id = guest.person().unwrap().id;

    admission::withdraw(&ctx, &owner, &Email("guest@example.com".into()))
        .await
        .unwrap();

    assert_eq!(
        admission::set_ownership(&ctx, &owner, guest_id, true).await.err(),
        Some(ServiceError::InvalidInput)
    );
}

/// Opening the server is a legitimate thing to want, and it must be something
/// somebody did rather than something that happened.
#[rstest]
#[tokio::test]
async fn an_owner_can_open_the_server_and_close_it_again(#[future(awt)] pool: SqlitePool) {
    use crate::models::user::Email;
    use crate::service::admission;

    let (ctx, owner, _) = a_server_with_an_owner(&pool).await;
    let stranger = Email("stranger@example.com".into());

    assert!(!admission::admits_email(&ctx, Some(&stranger)).await.unwrap());

    admission::set_open(&ctx, &owner, true).await.unwrap();
    assert!(admission::admits_email(&ctx, Some(&stranger)).await.unwrap());

    admission::set_open(&ctx, &owner, false).await.unwrap();
    assert!(!admission::admits_email(&ctx, Some(&stranger)).await.unwrap());
}

/// Withdrawing something nobody admitted is a mistake worth reporting, not a no-op:
/// the owner is looking at a list and expected that row to be on it.
#[rstest]
#[tokio::test]
async fn withdrawing_an_address_that_was_never_admitted(#[future(awt)] pool: SqlitePool) {
    use crate::models::user::Email;
    use crate::service::admission;

    let (ctx, owner, _) = a_server_with_an_owner(&pool).await;

    assert_eq!(
        admission::withdraw(&ctx, &owner, &Email("nobody@example.com".into()))
            .await
            .err(),
        Some(ServiceError::NotFound)
    );
}

// ---------------------------------------------------------------------------
// Claiming a server nobody owns
// ---------------------------------------------------------------------------

/// A server as it arrives: closed, unclaimed, and offering the code from its log.
fn unclaimed(pool: &SqlitePool, code: &str) -> Ctx {
    Ctx::new(pool.clone()).awaiting_claim(code.to_string())
}

async fn make_it_fresh(ctx: &Ctx) {
    sqlx::raw_sql("UPDATE server SET admits_anyone = 0, claimed_at = NULL")
        .execute(&ctx.db)
        .await
        .unwrap();
}

async fn claim_as(ctx: &Ctx, code: &str, who: &str) -> Result<Actor, ServiceError> {
    use crate::models::user;
    use crate::service::admission;

    admission::claim(
        ctx,
        code,
        "google",
        user::Sub(who.into()),
        None,
        Some(user::Email(format!("{who}@example.com"))),
    )
    .await
}

/// A1: the first person through the door owns the server, and can then use it.
#[rstest]
#[tokio::test]
async fn claiming_makes_the_first_person_the_owner(#[future(awt)] pool: SqlitePool) {
    use crate::service::{admission, identity};

    let ctx = unclaimed(&pool, "ABCD-2345");
    make_it_fresh(&ctx).await;

    let owner = claim_as(&ctx, "ABCD-2345", "me").await.unwrap();
    let id = owner.person().unwrap().id;

    assert!(admission::is_owner(&ctx, id).await.unwrap());
    // Admitted too, or the owner is an owner who cannot sign in.
    assert!(admission::admits_user(&ctx, id).await.unwrap());
    assert!(
        identity::from_session(&ctx, id.0).await.unwrap().is_some(),
        "the owner could not use the server they just claimed"
    );
}

/// A2, and the reason the code exists. Without it, anybody who can reach the port
/// during the gap between starting the process and claiming it becomes the owner —
/// and the person it happens to is simply refused from their own server.
#[rstest]
#[tokio::test]
async fn the_wrong_code_claims_nothing(#[future(awt)] pool: SqlitePool) {
    use crate::models::admission::Server;

    let ctx = unclaimed(&pool, "ABCD-2345");
    make_it_fresh(&ctx).await;

    assert_eq!(
        claim_as(&ctx, "WXYZ-9876", "stranger").await.err(),
        Some(ServiceError::Forbidden)
    );
    assert!(!Server::is_claimed(&ctx.db).await.unwrap());
}

/// A process offering no code cannot be claimed at all, which is the safe default and
/// the state of every server that already has an owner.
#[rstest]
#[tokio::test]
async fn a_server_offering_no_code_cannot_be_claimed(#[future(awt)] pool: SqlitePool) {
    let ctx = Ctx::new(pool.clone());
    make_it_fresh(&ctx).await;

    assert_eq!(
        claim_as(&ctx, "ABCD-2345", "stranger").await.err(),
        Some(ServiceError::Forbidden)
    );
}

/// The code is not a way in twice. Somebody who reads it off a log later must not be
/// able to take a server that already has an owner.
#[rstest]
#[tokio::test]
async fn a_claimed_server_cannot_be_claimed_again(#[future(awt)] pool: SqlitePool) {
    use crate::service::admission;

    let ctx = unclaimed(&pool, "ABCD-2345");
    make_it_fresh(&ctx).await;

    let first = claim_as(&ctx, "ABCD-2345", "me").await.unwrap();

    assert_eq!(
        claim_as(&ctx, "ABCD-2345", "stranger").await.err(),
        Some(ServiceError::Forbidden)
    );
    assert_eq!(
        admission::listing(&ctx, &first).await.unwrap().len(),
        1,
        "the second claim admitted somebody anyway"
    );
}

/// Codes differ between runs, so one read off an old log is not a key to a server
/// that has since restarted.
#[test]
fn a_claim_code_is_readable_and_not_reused() {
    use crate::service::admission::new_claim_code;

    let code = new_claim_code();
    assert_eq!(code.len(), 9, "{code}");
    assert_eq!(code.chars().nth(4), Some('-'));
    assert!(
        code.chars().all(|c| c == '-' || "23456789BCDFGHJKMNPQRSTVWXYZ".contains(c)),
        "a character that is easy to mistype: {code}"
    );
    assert_ne!(new_claim_code(), new_claim_code());
}

/// The bug this is here to stop coming back: a server seeded from `ALLOWED_EMAILS`
/// made its earliest user an owner and never bound their address, and `admits_user`
/// only looked at bindings — so the server refused the person it had just handed
/// itself to, permanently, with no way back that did not involve `sqlite3`.
#[rstest]
#[tokio::test]
async fn a_seeded_server_lets_its_owner_in(#[future(awt)] pool: SqlitePool) {
    use crate::models::user;
    use crate::service::admission::{self, Admission};
    use crate::service::identity;

    let ctx = Ctx::new(pool.clone());

    // Somebody who signed in before any of this existed.
    let before = identity::from_claims(
        &ctx,
        "google",
        user::Sub("me".into()),
        None,
        Some(user::Email("me@example.com".into())),
    )
    .await
    .unwrap();
    let id = before.person().unwrap().id;

    sqlx::raw_sql("UPDATE server SET admits_anyone = 0, claimed_at = NULL; DELETE FROM admitted_emails;")
        .execute(&ctx.db)
        .await
        .unwrap();

    admission::seed(&ctx, Some(&Admission::parse("me@example.com").unwrap()))
        .await
        .unwrap();

    assert!(admission::is_owner(&ctx, id).await.unwrap());
    assert!(
        admission::admits_user(&ctx, id).await.unwrap(),
        "the server handed itself to somebody it then refused to let in"
    );
    assert!(identity::from_session(&ctx, id.0).await.unwrap().is_some());
}

/// The same rule from the other side: an address admitted while somebody was away
/// works the moment they come back, without waiting for a sign-in to bind it.
#[rstest]
#[tokio::test]
async fn an_admitted_address_works_before_anything_bound_it(#[future(awt)] pool: SqlitePool) {
    use crate::models::admission::Admitted;
    use crate::models::user;
    use crate::service::{admission, identity};

    let ctx = Ctx::new(pool.clone());
    let person = identity::from_claims(
        &ctx,
        "google",
        user::Sub("her".into()),
        None,
        Some(user::Email("her@example.com".into())),
    )
    .await
    .unwrap();
    let id = person.person().unwrap().id;

    // Closed, and admitted by an address with nothing bound to it.
    sqlx::raw_sql("UPDATE server SET admits_anyone = 0; DELETE FROM admitted_emails;")
        .execute(&ctx.db)
        .await
        .unwrap();
    Admitted::seed(&ctx.db, &user::Email("her@example.com".into()), None)
        .await
        .unwrap();

    assert!(admission::admits_user(&ctx, id).await.unwrap());

    // And withdrawing it still refuses them, which is the property that must survive
    // making this more forgiving.
    Admitted::remove(&ctx.db, &user::Email("her@example.com".into()))
        .await
        .unwrap();
    assert!(!admission::admits_user(&ctx, id).await.unwrap());
}

/// `2 kg apples` once, then `apples`, is two kilos again.
///
/// The memory already held the unit, so `apples` came back in kilos and then asked how
/// many -- every week, for something bought two kilos at a time every week. This is the
/// other half of the same idea and the half somebody notices.
#[rstest]
#[tokio::test]
async fn a_remembered_item_returns_with_how_much(
    #[with(crate::models::fixtures::UNITS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;

    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "2 kg apples")
        .await
        .unwrap();
    // Removed, so the next line makes a row rather than finding this one -- what is
    // under test is the memory, not the merging.
    items::delete(&s.ctx, &s.mine, first.id).await.unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "apples")
        .await
        .unwrap();

    assert_eq!(again.amount, item::Amount(2.0), "how much was forgotten");
}

/// A number on the line is somebody stating one, and outranks the memory.
#[rstest]
#[tokio::test]
async fn a_stated_amount_beats_the_remembered_one(
    #[with(crate::models::fixtures::UNITS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = scene(pool).await;

    let first = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "2 kg apples")
        .await
        .unwrap();
    items::delete(&s.ctx, &s.mine, first.id).await.unwrap();

    let again = items::quick_add(&s.ctx, &s.mine, s.list.id, None, "1 kg apples")
        .await
        .unwrap();

    assert_eq!(again.amount, item::Amount(1.0));
}

// MARK: - `POST /api/sync`, which was in none of the above
//
// `sync::replay` was absent from this file entirely, and the two rules it breaks
// deliberately -- a resend is answered before the access check, and making a list has
// no list to check a role against -- are exactly the two places a stranger got in. The
// file's promise is that missing coverage is countable; this is the count.

/// A resend is a no-op for the person who sent it. For anybody else it is a guess.
///
/// `remembered` matched on the operation id alone and then answered with a row named
/// by the *incoming* payload, so a stranger could take an id they had used once, resend
/// it carrying somebody else's item uuid, and read that item back out of the reply --
/// before the access check, which is the one part of this route that is meant to run
/// early. Removed collaborators keep every uuid their last read gave them.
#[rstest]
#[tokio::test]
async fn a_resend_never_reads_a_row_for_somebody_else(
    #[future(awt)] pool: SqlitePool,
) {
    use crate::service::sync::{self, Operation, Outcome, Refusal, What};
    use time::OffsetDateTime;

    let s = scene(pool).await;
    let stranger_list = lists::create(&s.ctx, &s.theirs, list::Name("Theirs".into()))
        .await
        .unwrap();

    let id = format!("{:->36}", "shared-id");
    let op = |uuid: list::Uuid, what| Operation {
        id: id.clone(),
        at: OffsetDateTime::now_utc(),
        list: uuid,
        what,
    };

    // The stranger spends the id legitimately, on their own list.
    let first = sync::replay(
        &s.ctx,
        &s.theirs,
        vec![op(stranger_list.uuid.clone(), What::ClearDone { items: vec![] })],
    )
    .await
    .unwrap();
    assert!(
        matches!(first[0].outcome, Outcome::Applied { .. }),
        "the stranger could not use their own list"
    );

    // Now the owner resends *that* id. Whatever happens, it must not be answered from
    // a memory that belongs to somebody else.
    let second = sync::replay(
        &s.ctx,
        &s.mine,
        vec![op(
            s.list.uuid.clone(),
            What::SetDone { item: s.item.uuid.clone(), done: true },
        )],
    )
    .await
    .unwrap();

    assert_eq!(
        second[0].outcome,
        Outcome::Refused { why: Refusal::Invalid },
        "an id already minted by somebody else was treated as this person's own resend"
    );
}

/// Guessing a uuid is not a way into anybody's shopping — which is what `make_list`
/// said while returning the list to whoever asked.
///
/// The write that followed was refused, so nothing could be changed; the name, id,
/// owner and dates had already gone back in the reply. `ListGone` and not `NotAllowed`,
/// because `NotAllowed` confirms the list exists, which is the one fact a guess wants.
#[rstest]
#[tokio::test]
async fn making_a_list_that_is_already_somebody_elses_says_gone(
    #[future(awt)] pool: SqlitePool,
) {
    use crate::service::sync::{self, Operation, Outcome, Refusal, What};
    use time::OffsetDateTime;

    let s = scene(pool).await;

    let answers = sync::replay(
        &s.ctx,
        &s.theirs,
        vec![Operation {
            id: format!("{:->36}", "guess"),
            at: OffsetDateTime::now_utc(),
            list: s.list.uuid.clone(),
            what: What::MakeList { name: list::Name("Anything".into()) },
        }],
    )
    .await
    .unwrap();

    assert_eq!(
        answers[0].outcome,
        Outcome::Refused { why: Refusal::ListGone },
        "a guessed uuid was answered with somebody else's list"
    );
}

/// The half that must keep working: a device re-creating a list it made itself.
///
/// This is what `make_list` is idempotent *for*, and a fix that broke it would turn
/// every lost reply into a duplicate list.
#[rstest]
#[tokio::test]
async fn remaking_your_own_list_still_finds_it(#[future(awt)] pool: SqlitePool) {
    use crate::service::sync::{self, Operation, Outcome, What};
    use time::OffsetDateTime;

    let s = scene(pool).await;

    let answers = sync::replay(
        &s.ctx,
        &s.mine,
        vec![Operation {
            id: format!("{:->36}", "resend"),
            at: OffsetDateTime::now_utc(),
            list: s.list.uuid.clone(),
            what: What::MakeList { name: list::Name("Fruit & veg".into()) },
        }],
    )
    .await
    .unwrap();

    let Outcome::Applied { list: Some(found), .. } = &answers[0].outcome else {
        panic!("a device re-creating its own list was refused: {:?}", answers[0].outcome);
    };
    assert_eq!(found.id, s.list.id, "it made a second list instead of finding the first");
}
