//! What a batch does when it lands, including the cases nobody enjoys.
//!
//! Every rule in `docs/offline.md` that this route is responsible for has a test here,
//! and each is written as the situation it comes from rather than as the rule it
//! enforces — a test that reads like the rule is a test that passes because it repeats
//! the code.

use rstest::rstest;
use sqlx::SqlitePool;
use time::OffsetDateTime;

use super::sync::{self, Operation, Outcome, Refusal, What};
use super::{Actor, Ctx, items, lists};
use crate::models::item::{self, Amount, Item, Name};
use crate::models::list::{self, Role};
use crate::models::pool;
use crate::models::user;

/// Two people who share one list, which is the shape every conflict needs.
struct Two {
    ctx: Ctx,
    anna: Actor,
    ben: Actor,
    list: list::List,
}

async fn person(pool: &SqlitePool, sub: &str) -> Actor {
    let user = user::User::find_or_create(
        pool,
        user::Sub(sub.into()),
        Some(user::Name(sub.into())),
        Some(user::Email(format!("{sub}@example.com"))),
    )
    .await
    .unwrap();
    Actor::User(user)
}

async fn two(pool: SqlitePool) -> Two {
    let ctx = Ctx::new(pool.clone());
    let anna = person(&pool, "anna").await;
    let ben = person(&pool, "ben").await;

    let list = lists::create(&ctx, &anna, list::Name("Home".into()))
        .await
        .unwrap();

    let token = lists::invite(&ctx, &anna, list.id, Role::Editor).await.unwrap();
    lists::join(&ctx, &ben, &token).await.unwrap();

    Two { ctx, anna, ben, list }
}

impl Two {
    /// Something on the list, added the ordinary way.
    async fn add(&self, name: &str) -> Item {
        items::create(
            &self.ctx,
            &self.anna,
            self.list.id,
            None,
            Name(name.into()),
            Amount(1.0),
            None,
        )
        .await
        .unwrap()
    }

    async fn rows(&self) -> Vec<Item> {
        items::for_list(
            &self.ctx,
            &self.anna,
            self.list.id,
            super::everything(),
            crate::models::OrderBy {
                field: item::Field::Id,
                direction: crate::models::Direction::Ascending,
            },
        )
        .await
        .unwrap()
        .items
    }

    fn op(&self, id: &str, what: What) -> Operation {
        Operation {
            id: uuid(id),
            at: OffsetDateTime::now_utc(),
            list: self.list.uuid.clone(),
            what,
        }
    }
}

/// A readable stand-in for a minted operation id: the table wants 36 characters.
fn uuid(seed: &str) -> String {
    format!("{seed:->36}")
}

// ----------------------------------------------------------------- the ordinary case

/// A batch of things done in a shop lands, in order, and says what each became.
///
/// Seeded with the units, because this is the one test that leans on `2 kg apples`
/// being read the way a person means it -- and `kg` is only a unit if the table says so.
#[rstest]
#[tokio::test]
async fn a_batch_is_applied_in_order(
    #[with(crate::models::fixtures::UNITS)]
    #[future(awt)]
    pool: SqlitePool,
) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let named = item::Uuid::mint();

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![
            s.op(
                "add",
                What::Add {
                    item: named.clone(),
                    line: Some("2 kg apples".into()),
                    name: None,
                    amount: Amount(1.0),
                    unit: None,
                },
            ),
            s.op(
                "tick",
                What::SetDone {
                    item: milk.uuid.clone(),
                    done: true,
                },
            ),
        ],
    )
    .await
    .unwrap();

    assert!(matches!(answers[0].outcome, Outcome::Applied { .. }));
    assert!(matches!(answers[1].outcome, Outcome::Applied { .. }));

    // The row the device created is handed back, which is the only way it can learn
    // the id it did not have when it made the row.
    let Outcome::Applied { item: Some(added), .. } = &answers[0].outcome else {
        panic!("no row came back for the add");
    };
    assert_eq!(added.uuid, named, "the device's name for it was kept");
    assert_eq!(added.name, Name("Apples".into()));
    assert_eq!(added.amount, Amount(2.0));

    let rows = s.rows().await;
    assert!(rows.iter().any(|r| r.name == Name("Apples".into())));
    assert!(rows.iter().find(|r| r.id == milk.id).unwrap().done_at.is_some());
}

/// Sending the same batch twice does nothing the second time.
///
/// Which is what a lost answer produces: the device sent it, the server applied it,
/// and the reply never arrived. A resend is the device asking again, not asking twice.
#[rstest]
#[tokio::test]
async fn a_resend_changes_nothing(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let batch = || {
        vec![s.op(
            "add",
            What::Add {
                item: item::Uuid::mint(),
                line: Some("Bread".into()),
                name: None,
                amount: Amount(1.0),
                unit: None,
            },
        )]
    };

    // The same operation id both times; the item uuid inside differs, which is the
    // point -- it is the operation's name that makes this a resend, not the row's.
    let first = sync::replay(&s.ctx, &s.ben, batch()).await.unwrap();
    let again = sync::replay(&s.ctx, &s.ben, batch()).await.unwrap();

    assert!(matches!(first[0].outcome, Outcome::Applied { .. }));
    assert!(matches!(again[0].outcome, Outcome::AlreadyApplied { .. }));
    assert_eq!(
        s.rows().await.iter().filter(|r| r.name == Name("Bread".into())).count(),
        1,
        "the second send added a second row"
    );
}

// -------------------------------------------------------------------- delete is final

/// Anna deletes Milk. Ben, offline, ticks it off and renames it.
///
/// Both are dropped, and both come back saying why. Delete is final: a deletion is a
/// fact about the server, not an intention held on a device -- docs/offline.md (2).
#[rstest]
#[tokio::test]
async fn a_tick_on_a_deleted_row_is_refused_and_says_so(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    items::delete(&s.ctx, &s.anna, milk.id).await.unwrap();

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![
            s.op(
                "tick",
                What::SetDone {
                    item: milk.uuid.clone(),
                    done: true,
                },
            ),
            s.op(
                "edit",
                What::Update {
                    item: milk.uuid.clone(),
                    name: Name("Whole milk".into()),
                    amount: Amount(1.0),
                    unit: None,
                    seen: None,
                },
            ),
        ],
    )
    .await
    .unwrap();

    assert_eq!(answers[0].outcome, Outcome::Refused { why: Refusal::Gone });
    assert_eq!(answers[1].outcome, Outcome::Refused { why: Refusal::Gone });
    assert!(s.rows().await.is_empty(), "the row came back from the dead");
}

/// One refusal must not discard the rest.
///
/// Somebody who ticked six things off and edited a seventh that had been deleted
/// should lose the seventh, not all seven.
#[rstest]
#[tokio::test]
async fn a_refusal_does_not_stop_the_batch(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let gone = s.add("Milk").await;
    let here = s.add("Bread").await;
    items::delete(&s.ctx, &s.anna, gone.id).await.unwrap();

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![
            s.op("dead", What::SetDone { item: gone.uuid.clone(), done: true }),
            s.op("live", What::SetDone { item: here.uuid.clone(), done: true }),
        ],
    )
    .await
    .unwrap();

    assert_eq!(answers[0].outcome, Outcome::Refused { why: Refusal::Gone });
    assert!(matches!(answers[1].outcome, Outcome::Applied { .. }));
    assert!(
        s.rows().await.iter().find(|r| r.id == here.id).unwrap().done_at.is_some(),
        "the change behind the refusal was dropped too"
    );
}

/// Deleting something already deleted is what the operation wanted.
///
/// Refusing it would tell somebody their delete failed when the row is exactly as they
/// meant to leave it.
#[rstest]
#[tokio::test]
async fn deleting_what_has_already_gone_is_success(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    items::delete(&s.ctx, &s.anna, milk.id).await.unwrap();

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op("del", What::Delete { item: milk.uuid.clone() })],
    )
    .await
    .unwrap();

    assert_eq!(answers[0].outcome, Outcome::Applied { item: None, list: None });
}

// ------------------------------------------------------------------- add is idempotent

/// Anna adds `2 kg apples`. Ben, offline, adds it too.
///
/// One row, and it does not become 4 kg. Somebody adding a thing has not looked at the
/// amount -- docs/offline.md (1).
#[rstest]
#[tokio::test]
async fn two_people_adding_the_same_thing_make_one_row(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    items::quick_add(&s.ctx, &s.anna, s.list.id, None, "2 kg apples")
        .await
        .unwrap();

    sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "add",
            What::Add {
                item: item::Uuid::mint(),
                line: Some("2 kg apples".into()),
                name: None,
                amount: Amount(1.0),
                unit: None,
            },
        )],
    )
    .await
    .unwrap();

    let rows = s.rows().await;
    assert_eq!(rows.len(), 1, "two rows for one intention");
    assert_eq!(rows[0].amount, Amount(2.0), "the amount was added up");
}

// ------------------------------------------------------------------------- the clock

/// A tick is stamped with when the device says it happened, not when it arrived.
///
/// The whole reason this route exists rather than a replay through the REST ones: a
/// queue that sat in a pocket for an hour was claiming the shopping was done an hour
/// after it was.
#[rstest]
#[tokio::test]
async fn a_tick_keeps_the_time_it_was_made(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let an_hour_ago = OffsetDateTime::now_utc() - time::Duration::hours(1);

    let mut operation = s.op("tick", What::SetDone { item: milk.uuid.clone(), done: true });
    operation.at = an_hour_ago;

    sync::replay(&s.ctx, &s.ben, vec![operation]).await.unwrap();

    let done_at = s.rows().await[0].done_at.unwrap().0;
    assert!(
        (done_at - an_hour_ago).abs() < time::Duration::seconds(2),
        "stamped {done_at}, expected about {an_hour_ago}"
    );
}

/// A clock set to next year does not get to say the shopping was done next year.
///
/// Behind is believed and unbounded -- a phone in a drawer for a month is telling the
/// truth. Ahead is not: nothing that has happened can have happened after it arrived.
#[rstest]
#[tokio::test]
async fn a_clock_that_runs_fast_is_pulled_back(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let next_year = OffsetDateTime::now_utc() + time::Duration::days(365);

    let mut operation = s.op("tick", What::SetDone { item: milk.uuid.clone(), done: true });
    operation.at = next_year;

    sync::replay(&s.ctx, &s.ben, vec![operation]).await.unwrap();

    let done_at = s.rows().await[0].done_at.unwrap().0;
    assert!(done_at < next_year - time::Duration::days(1), "stamped {done_at}");
}

// ------------------------------------------------------------------------- the sweep

/// Ben sweeps two rows in a shop. Anna ticks a third off meanwhile.
///
/// Ben's sweep names the two it meant, so Anna's third survives -- docs/offline.md (4).
#[rstest]
#[tokio::test]
async fn a_sweep_takes_only_what_the_device_could_see(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let bread = s.add("Bread").await;
    let milk = s.add("Milk").await;
    let eggs = s.add("Eggs").await;

    for row in [&bread, &milk] {
        items::set_done(&s.ctx, &s.ben, row.id, true).await.unwrap();
    }
    // Anna, at home, after Ben pressed the button.
    items::set_done(&s.ctx, &s.anna, eggs.id, true).await.unwrap();

    sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "sweep",
            What::ClearDone {
                items: vec![bread.uuid.clone(), milk.uuid.clone()],
            },
        )],
    )
    .await
    .unwrap();

    let left: Vec<_> = s.rows().await.into_iter().map(|r| r.id).collect();
    assert_eq!(left, vec![eggs.id], "somebody else's tick was swept away");
}

// -------------------------------------------------------------------------- the rename

/// Anna sets Milk to 5. Ben, offline, renames the copy that still said 1.
///
/// Both survive -- docs/offline.md (5).
#[rstest]
#[tokio::test]
async fn a_rename_against_a_changed_row_splits_it(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;

    items::update(
        &s.ctx,
        &s.anna,
        milk.id,
        Name("Milk".into()),
        Amount(5.0),
        milk.unit_id,
        None,
    )
    .await
    .unwrap();

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "rename",
            What::Update {
                item: milk.uuid.clone(),
                name: Name("Whole milk".into()),
                amount: Amount(1.0),
                unit: milk.unit_id,
                seen: Some(items::Seen {
                    name: Name("Milk".into()),
                    amount: Amount(1.0),
                    unit_id: milk.unit_id,
                }),
            },
        )],
    )
    .await
    .unwrap();

    let Outcome::Applied { item: Some(renamed), .. } = &answers[0].outcome else {
        panic!("the rename did not apply");
    };
    assert_ne!(renamed.id, milk.id);
    assert_eq!(renamed.amount, Amount(1.0), "it took the other person's number");

    let rows = s.rows().await;
    assert_eq!(rows.len(), 2, "one of the two edits was lost");
    assert_eq!(rows[0].amount, Amount(5.0));
}

// -------------------------------------------------------------------------- losing access

/// Ben is removed at 14:00 and reaches signal at 14:30, with edits stamped 13:50.
///
/// Refused, whatever his phone says about when he did the work: access is decided by
/// arrival, never by the removed device's clock -- docs/offline.md (8).
#[rstest]
#[tokio::test]
async fn work_from_somebody_removed_is_refused_however_it_is_stamped(
    #[future(awt)] pool: SqlitePool,
) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;

    let ben = s.ben.person().unwrap().id;
    lists::remove_member(&s.ctx, &s.anna, s.list.id, ben).await.unwrap();

    let mut operation = s.op("tick", What::SetDone { item: milk.uuid.clone(), done: true });
    operation.at = OffsetDateTime::now_utc() - time::Duration::minutes(40);

    let answers = sync::replay(&s.ctx, &s.ben, vec![operation]).await.unwrap();

    assert_eq!(answers[0].outcome, Outcome::Refused { why: Refusal::NotAllowed });
    assert!(s.rows().await[0].done_at.is_none(), "a removed person still wrote");
}

/// A refused operation is not remembered as applied.
///
/// If Ben is invited back, the work his phone kept must still land -- and it cannot if
/// the server thinks it has already seen it.
#[rstest]
#[tokio::test]
async fn refused_work_lands_if_they_are_invited_back(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let ben = s.ben.person().unwrap().id;

    lists::remove_member(&s.ctx, &s.anna, s.list.id, ben).await.unwrap();
    let operation = s.op("tick", What::SetDone { item: milk.uuid.clone(), done: true });
    sync::replay(&s.ctx, &s.ben, vec![operation.clone()]).await.unwrap();

    let token = lists::invite(&s.ctx, &s.anna, s.list.id, Role::Editor).await.unwrap();
    lists::join(&s.ctx, &s.ben, &token).await.unwrap();

    let answers = sync::replay(&s.ctx, &s.ben, vec![operation]).await.unwrap();

    assert!(matches!(answers[0].outcome, Outcome::Applied { .. }));
    assert!(s.rows().await[0].done_at.is_some());
}

// ----------------------------------------------------------------------- a list that went

/// A fortnight of changes for a list somebody deleted. They cannot be applied and never
/// will be -- docs/offline.md (9). Dropped, and said.
#[rstest]
#[tokio::test]
async fn changes_for_a_list_that_has_gone_are_refused(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let operation = s.op("tick", What::SetDone { item: milk.uuid.clone(), done: true });

    lists::delete(&s.ctx, &s.anna, s.list.id).await.unwrap();

    let answers = sync::replay(&s.ctx, &s.ben, vec![operation]).await.unwrap();

    assert_eq!(answers[0].outcome, Outcome::Refused { why: Refusal::ListGone });
}

// ------------------------------------------------------------------------------- aisles

/// Ben, with no signal, files milk under an aisle. It arrives later.
///
/// Filing was the last thing a device could only do with a connection, which on a
/// device with no server at all meant never -- docs/offline.md.
#[rstest]
#[tokio::test]
async fn a_tag_filed_offline_arrives(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let dairy = a_tag(&s, "dairy").await;

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "file",
            What::Tag { item: milk.uuid.clone(), tag: dairy, attached: true },
        )],
    )
    .await
    .unwrap();

    assert!(
        matches!(answers[0].outcome, Outcome::Applied { .. }),
        "filing was refused: {:?}",
        answers[0].outcome
    );
    assert_eq!(tags_on(&s, milk.id).await, vec![dairy], "not filed");
}

/// And unfiling, which is the same operation with the flag the other way.
#[rstest]
#[tokio::test]
async fn a_tag_taken_off_offline_arrives(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let dairy = a_tag(&s, "dairy").await;
    super::tags::attach(&s.ctx, &s.anna, milk.id, dairy).await.unwrap();

    sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "unfile",
            What::Tag { item: milk.uuid.clone(), tag: dairy, attached: false },
        )],
    )
    .await
    .unwrap();

    assert!(tags_on(&s, milk.id).await.is_empty(), "still filed");
}

/// A tag id that is not a tag.
///
/// Invalid rather than gone, and the difference matters to the device: `Gone` means the
/// row went and the operation is dropped, `Invalid` means the operation was never going
/// to work. Neither is worth a retry, and only one of them is true.
#[rstest]
#[tokio::test]
async fn filing_under_an_aisle_that_does_not_exist_is_invalid(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "file",
            What::Tag { item: milk.uuid.clone(), tag: crate::models::tag::Id(9_999), attached: true },
        )],
    )
    .await
    .unwrap();

    assert_eq!(answers[0].outcome, Outcome::Refused { why: Refusal::Invalid });
}

/// The same filing sent twice, which is what a device does when a reply goes missing.
#[rstest]
#[tokio::test]
async fn filing_the_same_thing_twice_is_harmless(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let milk = s.add("Milk").await;
    let dairy = a_tag(&s, "dairy").await;
    let operation =
        s.op("file", What::Tag { item: milk.uuid.clone(), tag: dairy, attached: true });

    sync::replay(&s.ctx, &s.ben, vec![operation.clone()]).await.unwrap();
    let again = sync::replay(&s.ctx, &s.ben, vec![operation]).await.unwrap();

    assert!(
        matches!(again[0].outcome, Outcome::AlreadyApplied { .. }),
        "a resend was treated as new work: {:?}",
        again[0].outcome
    );
    assert_eq!(tags_on(&s, milk.id).await, vec![dairy], "filed twice over");
}

async fn a_tag(s: &Two, name: &str) -> crate::models::tag::Id {
    // System, because aisles are the server's reference data rather than
    // anybody's to invent -- see `tags::writable`.
    super::tags::create(&s.ctx, &Actor::System, crate::models::tag::Name(name.into()), None, None)
        .await
        .unwrap()
        .id
}

async fn tags_on(s: &Two, item: item::Id) -> Vec<crate::models::tag::Id> {
    crate::models::tag::Tag::for_item(&s.ctx.db, item)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect()
}

/// Ben, with no signal, changes the order he walks the shop in.
///
/// Per person, so it can be last-write-wins without anybody else being affected --
/// docs/offline.md's table says so and this is what makes it true offline as well.
#[rstest]
#[tokio::test]
async fn a_walking_order_set_offline_arrives(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let dairy = a_tag(&s, "dairy").await;
    let bakery = a_tag(&s, "bakery").await;

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op("order", What::SetTagOrder { tags: vec![bakery, dairy] })],
    )
    .await
    .unwrap();

    assert!(
        matches!(answers[0].outcome, Outcome::Applied { .. }),
        "the order was refused: {:?}",
        answers[0].outcome
    );

    let walked = super::tags::order_for(&s.ctx, &s.ben, s.list.id)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect::<Vec<_>>();
    assert_eq!(walked.first(), Some(&bakery), "bakery was moved to the front");
}

/// Somebody who has set their own walk keeps it when another person changes theirs.
///
/// The reason this one is safe to resolve by last-write-wins. Not because an order
/// cannot reach anybody else -- it can: `order_for` falls back to whatever the list
/// has when a person has never set one, which is how a second shopper starts out
/// somewhere sensible rather than alphabetical. But an order somebody *has* chosen is
/// theirs, and nobody else's queue can overwrite it.
#[rstest]
#[tokio::test]
async fn one_persons_walk_does_not_overwrite_anothers(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;
    let dairy = a_tag(&s, "dairy").await;
    let bakery = a_tag(&s, "bakery").await;

    // Anna has said how she walks it.
    super::tags::set_order(&s.ctx, &s.anna, s.list.id, &[dairy, bakery])
        .await
        .unwrap();

    // Ben, offline, says the opposite.
    sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op("order", What::SetTagOrder { tags: vec![bakery, dairy] })],
    )
    .await
    .unwrap();

    let anna = super::tags::order_for(&s.ctx, &s.anna, s.list.id)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect::<Vec<_>>();
    assert_eq!(anna.first(), Some(&dairy), "Ben's walk overwrote Anna's");
}

/// A tag id that is not a tag, in an order.
#[rstest]
#[tokio::test]
async fn a_walking_order_naming_nothing_is_invalid(#[future(awt)] pool: SqlitePool) {
    let s = two(pool).await;

    let answers = sync::replay(
        &s.ctx,
        &s.ben,
        vec![s.op(
            "order",
            What::SetTagOrder { tags: vec![crate::models::tag::Id(9_999)] },
        )],
    )
    .await
    .unwrap();

    assert_eq!(answers[0].outcome, Outcome::Refused { why: Refusal::Invalid });
}

// MARK: - What the two `sync` authorization fixes mean for a shared list
//
// Both fixes turn on identity -- a resend belongs to its sender, a `MakeList` belongs
// to the list's owner -- and a shared list is where "belongs to" stops being obvious.
// A rule that reads correctly for one person and quietly refuses an editor is a worse
// bug than the one it replaced, so these say what happens.

/// An editor is not the owner, so a `MakeList` naming a list they were *invited* to is
/// refused — and that is right, because their device never made it and would never
/// queue this. What matters is that refusing it does not take the rest of the batch
/// down with it.
///
/// `replay` is in order and never skips, and every other operation names its list by
/// uuid and is checked on its own. So Ben's real work lands whether or not the
/// redundant `MakeList` in front of it does.
#[rstest]
#[tokio::test]
async fn a_refused_make_list_does_not_block_the_batch_behind_it(
    #[future(awt)] pool: SqlitePool,
) {
    let t = two(pool).await;

    let answers = sync::replay(
        &t.ctx,
        &t.ben,
        vec![
            t.op("make", What::MakeList { name: list::Name("Home".into()) }),
            t.op(
                "add",
                What::Add {
                    item: item::Uuid::mint(),
                    line: None,
                    name: Some(Name("Bread".into())),
                    amount: Amount(1.0),
                    unit: None,
                },
            ),
        ],
    )
    .await
    .unwrap();

    assert_eq!(
        answers[0].outcome,
        Outcome::Refused { why: Refusal::ListGone },
        "an editor was handed the owner's list back"
    );
    assert!(
        matches!(answers[1].outcome, Outcome::Applied { .. }),
        "a refused MakeList took the editor's real work with it: {:?}",
        answers[1].outcome
    );
    assert!(
        t.rows().await.iter().any(|row| row.name == Name("Bread".into())),
        "the row never reached the list"
    );
}

/// The owner's own resend still finds the list, on a list that happens to be shared.
///
/// Sharing must not change what idempotency means for the person who made it.
#[rstest]
#[tokio::test]
async fn the_owner_can_still_remake_a_list_they_have_shared(#[future(awt)] pool: SqlitePool) {
    let t = two(pool).await;

    let answers = sync::replay(
        &t.ctx,
        &t.anna,
        vec![t.op("remake", What::MakeList { name: list::Name("Home".into()) })],
    )
    .await
    .unwrap();

    let Outcome::Applied { list: Some(found), .. } = &answers[0].outcome else {
        panic!("the owner could not remake her own shared list: {:?}", answers[0].outcome);
    };
    assert_eq!(found.id, t.list.id, "a second list was made beside the shared one");
}

/// A resend belongs to whoever sent it, and on a shared list both people are sending.
///
/// Ben's own operation, resent by Ben, is still the no-op it was before the scoping --
/// the fix must not have made "somebody else's id" out of "the other person on my
/// list".
#[rstest]
#[tokio::test]
async fn an_editors_own_resend_is_still_a_no_op(#[future(awt)] pool: SqlitePool) {
    let t = two(pool).await;
    let bread = t.add("Bread").await;

    let tick = t.op("bens-tick", What::SetDone { item: bread.uuid.clone(), done: true });

    let first = sync::replay(&t.ctx, &t.ben, vec![tick.clone()]).await.unwrap();
    assert!(
        matches!(first[0].outcome, Outcome::Applied { .. }),
        "an editor could not cross something off: {:?}",
        first[0].outcome
    );

    let again = sync::replay(&t.ctx, &t.ben, vec![tick]).await.unwrap();
    let Outcome::AlreadyApplied { item: Some(row), .. } = &again[0].outcome else {
        panic!("an editor's resend stopped being a resend: {:?}", again[0].outcome);
    };
    assert_eq!(row.uuid, bread.uuid, "the wrong row came back");
    assert!(row.done_at.is_some(), "it came back as though the tick had not happened");
}

/// And the two people on one list do not share a memory of what has been applied.
///
/// Anna resending *Ben's* id is not Anna resending anything; it is an id that is
/// already spoken for. Refused rather than answered, so it can never become a way to
/// read a row through somebody else's operation.
#[rstest]
#[tokio::test]
async fn one_persons_resend_is_not_the_others(#[future(awt)] pool: SqlitePool) {
    let t = two(pool).await;
    let bread = t.add("Bread").await;

    let bens = t.op("shared-id", What::SetDone { item: bread.uuid.clone(), done: true });
    sync::replay(&t.ctx, &t.ben, vec![bens]).await.unwrap();

    let annas = t.op("shared-id", What::SetDone { item: bread.uuid.clone(), done: false });
    let answers = sync::replay(&t.ctx, &t.anna, vec![annas]).await.unwrap();

    assert_eq!(
        answers[0].outcome,
        Outcome::Refused { why: Refusal::Invalid },
        "one person's applied-operation id was read as the other's resend"
    );
}

/// Filing something that is *already* filed, under a different operation id.
///
/// `filing_the_same_thing_twice_is_harmless` above resends the same operation id, which
/// `remembered` answers before any of this is reached — so the case that actually
/// happens was never covered. It happens whenever an answer is lost: the attach landed,
/// the reply did not, and the device queues it again with a fresh id. It also happens
/// when two people file the same row.
///
/// It used to raise `Conflict`, which is not a refusal of one operation but a failure
/// of the whole batch — a 409 for everything, on every retry, for ever.
#[rstest]
#[tokio::test]
async fn filing_what_is_already_filed_is_not_a_conflict(#[future(awt)] pool: SqlitePool) {
    let t = two(pool).await;
    let milk = t.add("Milk").await;
    let dairy = a_tag(&t, "dairy").await;

    let first = t.op("first", What::Tag { item: milk.uuid.clone(), tag: dairy, attached: true });
    sync::replay(&t.ctx, &t.anna, vec![first]).await.unwrap();

    // A different id: the same intention, queued again because the answer was lost.
    let again = t.op("second", What::Tag { item: milk.uuid.clone(), tag: dairy, attached: true });
    let answers = sync::replay(&t.ctx, &t.anna, vec![again]).await.unwrap();

    assert!(
        matches!(answers[0].outcome, Outcome::Applied { .. }),
        "already filed came back as {:?}",
        answers[0].outcome
    );
    assert_eq!(tags_on(&t, milk.id).await, vec![dairy], "filed twice over");
}

/// One operation the server cannot handle must not throw away the answers for the ones
/// that already landed.
///
/// This is what turned a single conflicting `attach_tag` into a queue that could never
/// drain: `replay` used `?`, so the whole batch came back as one error, the device was
/// never told that the operations before it had applied, and it sent all thirteen again
/// — hitting the same operation, getting the same nothing, for ever.
#[rstest]
#[tokio::test]
async fn what_landed_before_a_failure_is_still_reported(#[future(awt)] pool: SqlitePool) {
    let t = two(pool).await;
    let milk = t.add("Milk").await;

    let answers = sync::replay(
        &t.ctx,
        &t.anna,
        vec![
            t.op("tick", What::SetDone { item: milk.uuid.clone(), done: true }),
            // A tag id that names nothing. Refused rather than fatal -- but the point
            // of the test is the answer *before* it.
            t.op("file", What::Tag { item: milk.uuid.clone(), tag: crate::models::tag::Id(9_999), attached: true }),
        ],
    )
    .await
    .unwrap();

    assert!(
        matches!(answers[0].outcome, Outcome::Applied { .. }),
        "the tick that landed was not reported: {:?}",
        answers[0].outcome
    );
    assert!(
        t.rows().await.iter().any(|row| row.uuid == milk.uuid && row.done_at.is_some()),
        "it was reported as applied without being applied"
    );
}
