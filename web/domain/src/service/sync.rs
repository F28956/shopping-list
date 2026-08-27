//! Replaying what a device did while it could not reach the server.
//!
//! One route, because a batch is the unit a device has: everything it did since it was
//! last heard from, in the order it did it. See `docs/offline.md` for why each rule is
//! what it is; this module is where they are enforced.
//!
//! Three properties hold, and each is load-bearing:
//!
//! * **Atomic per operation, not per batch.** One refusal must not discard the rest. A
//!   person who ticked six things off and edited a seventh that somebody had deleted
//!   should lose the seventh, not all seven.
//! * **Idempotent.** Every operation carries a UUID the device minted, and applying one
//!   records it. A resend -- which is what a lost answer produces -- reports the
//!   operation as applied and does nothing.
//! * **Rejections are data.** An operation on a list somebody was removed from comes
//!   back with a reason the app can show, not as a status code that fails the batch.
//!
//! **Push only, for now.** The sketch in `docs/offline.md` had this route carrying
//! changes back as well, behind a cursor. It does not: the existing event streams are
//! how a client learns to re-read, and a second way to learn the same thing is a second
//! thing to keep in step. What does come back is the *row each operation produced*,
//! which is not news about other people -- it is the answer to "what did my own change
//! turn into", and a device that created a row offline has no other way to learn its id.

use time::OffsetDateTime;

use crate::models::item::{self, Amount, Item, Name};
use crate::models::list::{self, List};
use crate::models::tag;
use crate::models::unit;
use crate::models::user;

use super::{Actor, Ctx, Result, ServiceError, items, lists, tags};

/// How far ahead of the server a device's clock may be before it is pulled back.
///
/// Behind is fine and unbounded -- a queue never expires, and a phone left in a drawer
/// for a month is telling the truth about when it acted. Ahead is not: a clock set to
/// next year would win every conflict for a year, and nothing that has happened can
/// have happened after it arrived.
///
/// A minute of slack rather than none, because clocks disagree by seconds honestly and
/// clamping every one of them would make the ordering the server's rather than the
/// device's -- which is the thing `docs/offline.md` (7) decided against.
pub const CLOCK_SLACK_SECONDS: i64 = 60;

/// What one queued change says.
///
/// Rows are named by `uuid` throughout, never by `id`. That is the whole reason
/// `items.uuid` exists: a device that added something with no signal has no `id` for it
/// and never will until the server answers, but it has been calling it by this since
/// the moment somebody typed it.
#[derive(Debug, Clone)]
pub struct Operation {
    /// What this operation is called. Minted on the device; the memory of it is what
    /// makes a resend a no-op.
    pub id: String,
    /// When the device says it happened.
    pub at: OffsetDateTime,
    /// The list it was made against.
    pub list: list::Uuid,
    pub what: What,
}

/// The operations a device may queue.
///
/// Deliberately not every route. Invitations and joining are absent and always will be:
/// a share code is a secret the server issues, and an offline device cannot invent one.
#[derive(Debug, Clone)]
pub enum What {
    /// Make the list itself, under the name the device has been calling it by.
    ///
    /// The one operation that does not need the list to exist, and the reason it can
    /// be sent at all: a list started with no signal has no `id` and never will until
    /// this arrives. Everything else in this enum already names its list by `uuid`, so
    /// nothing else had to change for an offline list to be usable.
    ///
    /// Whoever sends it owns what it creates. There is no list to check a role
    /// against — making one is something any signed-in person may do, exactly as
    /// `POST /api/lists` is.
    MakeList { name: list::Name },
    /// Put this on the list. Idempotent by name, and by `uuid` on a resend.
    Add {
        item: item::Uuid,
        line: Option<String>,
        name: Option<Name>,
        amount: Amount,
        unit: Option<unit::Id>,
    },
    /// Cross it off, or put it back. `at` is the time it is stamped with, which is what
    /// this route exists to make possible -- the REST route stamps the time the request
    /// arrived, so a tick replayed an hour late claimed to have happened an hour late.
    SetDone { item: item::Uuid, done: bool },
    /// Correct what somebody typed. `seen` is what the row looked like on the device,
    /// and is what decides between renaming a row and splitting one -- see
    /// [`items::update`].
    Update {
        item: item::Uuid,
        name: Name,
        amount: Amount,
        unit: Option<unit::Id>,
        seen: Option<items::Seen>,
    },
    /// Take it off the list. Final: nothing that arrives afterwards brings it back.
    Delete { item: item::Uuid },
    /// Empty the trolley, of exactly the rows the device could see. See the service's
    /// [`items::clear_done`] for why it names them.
    ClearDone { items: Vec<item::Uuid> },
    /// File it under an aisle, or stop filing it there.
    ///
    /// These were the last two things a device could only do with a connection, which
    /// on a device with no server at all meant it could never do them. The tag is named
    /// by `id` rather than by name, and that is safe for the same reason the clients
    /// bundle `reference.json`: the ids in that file are the ids in the seed, so a
    /// phone that has never met a server still means aisle 5 by 5.
    Tag { item: item::Uuid, tag: tag::Id, attached: bool },
    /// The order this person walks this list in.
    ///
    /// Names no item, like `MakeList`. Last write by `at` wins, and that is safe
    /// because the row it writes is keyed by the person: somebody who has chosen an
    /// order keeps it whatever anybody else queues. It is not invisible to others --
    /// `tags::order_for` falls back to the list's when a person has never set one, so
    /// a second shopper starts somewhere sensible rather than alphabetical -- but it
    /// cannot overwrite a choice somebody made. See docs/offline.md's table.
    SetTagOrder { tags: Vec<tag::Id> },
}

impl What {
    /// A short, stable name, for the memory of what was applied.
    pub fn kind(&self) -> &'static str {
        match self {
            What::MakeList { .. } => "make_list",
            What::Add { .. } => "add",
            What::SetDone { .. } => "set_done",
            What::Update { .. } => "update",
            What::Delete { .. } => "delete",
            What::ClearDone { .. } => "clear_done",
            What::Tag { attached: true, .. } => "attach_tag",
            What::Tag { attached: false, .. } => "detach_tag",
            What::SetTagOrder { .. } => "set_tag_order",
        }
    }
}

/// What became of one operation.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// It was applied, and this is the row it produced.
    ///
    /// The row is here so a device can learn what it could not know: the `id` of
    /// something it created offline, and the row a rename split off. Without it, a
    /// queue of "add milk, tick milk off" could send the first and have nothing to
    /// name in the second.
    Applied {
        item: Option<Item>,
        /// The list a [`What::MakeList`] produced, for the same reason `item` is here:
        /// the device knows what it called the list and not what the server calls it.
        ///
        /// Absent on every other operation, and skipped rather than sent as null, so
        /// adding it changed nothing a client already reads.
        #[serde(skip_serializing_if = "Option::is_none")]
        list: Option<List>,
    },
    /// It was applied before, on an earlier send of the same batch. The row is looked
    /// up rather than remembered, so it is the row as it stands now.
    AlreadyApplied {
        item: Option<Item>,
        #[serde(skip_serializing_if = "Option::is_none")]
        list: Option<List>,
    },
    /// It will never apply, and the person should be told why.
    Refused { why: Refusal },
}

/// Why an operation was refused.
///
/// Data, not an error. Every one of these is a sentence an app can put in front of
/// somebody, and none of them is a reason to discard the rest of the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The row is gone. Delete is final: a tick or an edit that arrives after one has
    /// nothing to land on and never will.
    Gone,
    /// The list is gone. A queue never expires, so a device can arrive with a
    /// fortnight of changes for a list somebody deleted.
    ListGone,
    /// Not allowed to write to that list -- usually somebody removed since the change
    /// was made. Decided by arrival, never by the device's clock; see
    /// `docs/offline.md` (8).
    NotAllowed,
    /// The operation itself does not make sense: an amount of zero, an empty name.
    Invalid,
}

/// One operation's fate, as it goes back to the device that sent it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Applied {
    pub id: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// Replays a batch, in the order it was sent.
///
/// In order, and never skipping: the batch is one device's story of what it did, and
/// applying a later operation before an earlier one would tell that story wrong -- a
/// tick landing before the add that created the row it ticks.
///
/// A refusal does not stop the batch. It cannot: the operations behind it are about
/// other rows and other lists, and holding them because one row was deleted would turn
/// one lost change into all of them.
pub async fn replay(ctx: &Ctx, actor: &Actor, batch: Vec<Operation>) -> Result<Vec<Applied>> {
    let who = actor.person()?;
    let arrived = OffsetDateTime::now_utc();

    let mut answers = Vec::with_capacity(batch.len());
    for operation in batch {
        let id = operation.id.clone();
        let outcome = one(ctx, actor, who, operation, arrived).await?;
        answers.push(Applied { id, outcome });
    }
    Ok(answers)
}

async fn one(
    ctx: &Ctx,
    actor: &Actor,
    who: &user::User,
    operation: Operation,
    arrived: OffsetDateTime,
) -> Result<Outcome> {
    // Before anything else, and before any access check: a resend is a no-op even for
    // somebody who has since been removed from the list. They are not writing again --
    // the write already happened, while they still could.
    if let Some(before) = remembered(ctx, &operation).await? {
        return Ok(Outcome::AlreadyApplied { item: before.item, list: before.list });
    }

    // Making the list is answered before the list is looked up, because it is the one
    // operation for which not finding it is the point. There is no list to check a
    // role against either: making one is something any signed-in person may do,
    // exactly as `POST /api/lists` is.
    if let What::MakeList { name } = &operation.what {
        let made = make_list(ctx, who, &operation.list, name.clone()).await?;
        remember(ctx, &operation, who).await?;
        return Ok(Outcome::Applied { item: None, list: Some(made) });
    }

    // The list, by the name the device knows it by. Gone is a refusal rather than an
    // error: a queue never expires, so this is an ordinary thing to arrive.
    let Ok(list) = list::List::get(&ctx.db, list::Lookup::Uuid(operation.list.clone())).await
    else {
        return Ok(Outcome::Refused { why: Refusal::ListGone });
    };

    // Decided here, on arrival, and never from `operation.at`. A device can claim any
    // time it likes, so trusting one to police access is not a choice worth the safety
    // it costs -- docs/offline.md (8).
    if lists::editable(ctx, who, list.id).await.is_err() {
        return Ok(Outcome::Refused { why: Refusal::NotAllowed });
    }

    let at = clamp(operation.at, arrived);
    let outcome = apply(ctx, actor, &list, &operation.what, at).await?;

    if !matches!(outcome, Outcome::Refused { .. }) {
        remember(ctx, &operation, who).await?;
    }

    Ok(outcome)
}

/// Pulls a device's claim about when something happened back into plausibility.
///
/// Only forwards. See [`CLOCK_SLACK_SECONDS`].
pub fn clamp(claimed: OffsetDateTime, arrived: OffsetDateTime) -> OffsetDateTime {
    let ceiling = arrived + time::Duration::seconds(CLOCK_SLACK_SECONDS);
    if claimed > ceiling { arrived } else { claimed }
}

async fn apply(
    ctx: &Ctx,
    actor: &Actor,
    list: &list::List,
    what: &What,
    at: OffsetDateTime,
) -> Result<Outcome> {
    match what {
        // Answered in `one`, before the list is looked up — it is the operation for
        // which not finding the list is the point, so by here it cannot happen.
        What::MakeList { .. } => Ok(Outcome::Applied { item: None, list: None }),
        What::Add {
            item,
            line,
            name,
            amount,
            unit,
        } => {
            let added = match (line, name) {
                (Some(line), _) => {
                    items::quick_add(ctx, actor, list.id, Some(item.clone()), line).await
                }
                (None, Some(name)) => {
                    items::create(
                        ctx,
                        actor,
                        list.id,
                        Some(item.clone()),
                        name.clone(),
                        *amount,
                        *unit,
                    )
                    .await
                }
                (None, None) => Err(ServiceError::InvalidInput),
            };
            Ok(finish(added))
        }

        What::SetDone { item, done } => match find(ctx, item).await? {
            None => Ok(Outcome::Refused { why: Refusal::Gone }),
            // Stamped with when the device says it happened, not with when it arrived.
            // This is the whole reason this route exists rather than replaying through
            // the REST ones.
            Some(row) => Ok(finish(
                items::set_done_at(ctx, actor, row.id, *done, at).await,
            )),
        },

        What::Update {
            item,
            name,
            amount,
            unit,
            seen,
        } => match find(ctx, item).await? {
            None => Ok(Outcome::Refused { why: Refusal::Gone }),
            Some(row) => Ok(finish(
                items::update(ctx, actor, row.id, name.clone(), *amount, *unit, seen.clone()).await,
            )),
        },

        What::Delete { item } => match find(ctx, item).await? {
            // Already gone is the outcome the operation wanted. Refusing it would tell
            // somebody their delete failed when the row is exactly as they left it.
            None => Ok(Outcome::Applied { item: None, list: None }),
            Some(row) => match items::delete(ctx, actor, row.id).await {
                Ok(()) => Ok(Outcome::Applied { item: None, list: None }),
                Err(ServiceError::NotFound) => Ok(Outcome::Refused { why: Refusal::Gone }),
                Err(other) => Err(other),
            },
        },

        What::ClearDone { items: named } => {
            let mut ids = Vec::with_capacity(named.len());
            for uuid in named {
                if let Some(row) = find(ctx, uuid).await? {
                    ids.push(row.id);
                }
            }
            // Rows that have gone are simply absent from the list -- somebody deleting
            // one of them first is the same outcome by another route.
            items::clear_done(ctx, actor, list.id, Some(&ids)).await?;
            Ok(Outcome::Applied { item: None, list: None })
        }

        What::SetTagOrder { tags: order } => {
            match tags::set_order(ctx, actor, list.id, order).await {
                Ok(()) => Ok(Outcome::Applied { item: None, list: None }),
                // A tag id that is not a tag -- the same reading as filing under one.
                Err(ServiceError::NotFound | ServiceError::InvalidInput) => {
                    Ok(Outcome::Refused { why: Refusal::Invalid })
                }
                Err(ServiceError::Forbidden) => Ok(Outcome::Refused { why: Refusal::NotAllowed }),
                Err(other) => Err(other),
            }
        }

        What::Tag { item, tag, attached } => match find(ctx, item).await? {
            None => Ok(Outcome::Refused { why: Refusal::Gone }),
            Some(row) => {
                let done = if *attached {
                    tags::attach(ctx, actor, row.id, *tag).await
                } else {
                    tags::detach(ctx, actor, row.id, *tag).await
                };
                match done {
                    // The row rather than nothing, so the device learns what it is
                    // filed under now -- including any tag another device added while
                    // this one had no signal.
                    Ok(()) => Ok(finish(
                        Item::get(&ctx.db, item::Lookup::Uuid(item.clone()))
                            .await
                            .map_err(Into::into),
                    )),
                    // A tag id that is not a tag. Invalid rather than gone: the row is
                    // there, the aisle never was, and resending will not help. Both
                    // errors mean that -- the foreign key refuses it as invalid input,
                    // and a tag deleted since refuses it as not found.
                    Err(ServiceError::NotFound | ServiceError::InvalidInput) => {
                        Ok(Outcome::Refused { why: Refusal::Invalid })
                    }
                    Err(ServiceError::Forbidden) => Ok(Outcome::Refused { why: Refusal::NotAllowed }),
                    Err(other) => Err(other),
                }
            }
        },
    }
}

/// Turns a service answer into an outcome, keeping the refusals that are the device's
/// news rather than the server's fault.
fn finish(result: Result<Item>) -> Outcome {
    match result {
        Ok(item) => Outcome::Applied { item: Some(item), list: None },
        Err(ServiceError::NotFound) => Outcome::Refused { why: Refusal::Gone },
        Err(ServiceError::Forbidden) => Outcome::Refused { why: Refusal::NotAllowed },
        Err(ServiceError::InvalidInput) => Outcome::Refused { why: Refusal::Invalid },
        // Anything else is the server having a problem, not the person having one.
        Err(_) => Outcome::Refused { why: Refusal::Invalid },
    }
}

async fn find(ctx: &Ctx, uuid: &item::Uuid) -> Result<Option<Item>> {
    match Item::get(&ctx.db, item::Lookup::Uuid(uuid.clone())).await {
        Ok(item) => Ok(Some(item)),
        Err(crate::models::Error::NotFound) => Ok(None),
        Err(other) => Err(other.into()),
    }
}

/// Whether this operation has been applied before, and what its row looks like now.
/// Creates a list a device made with no signal, or finds the one it already made.
///
/// Idempotent by `uuid`, which matters more here than it looks: two devices of the
/// same person can queue the same list, and a resend after a reply that never arrived
/// is the ordinary case. Finding it rather than failing is what makes both harmless.
///
/// A uuid that belongs to somebody else's list is refused by not being theirs — the
/// lookup finds it, the ownership check does not match, and the device is told the
/// list is gone. Guessing a uuid is not a way into anybody's shopping.
async fn make_list(
    ctx: &Ctx,
    who: &user::User,
    uuid: &list::Uuid,
    name: list::Name,
) -> Result<List> {
    if let Ok(existing) = list::List::get(&ctx.db, list::Lookup::Uuid(uuid.clone())).await {
        return Ok(existing);
    }

    let made = List::create(&ctx.db, uuid.clone(), who.id, name).await?;

    // Told to the person rather than to the list, for the reason `lists::create`
    // gives: a list that has just been made has no watchers.
    ctx.changes.announce_lists_of(who.id);
    Ok(made)
}

async fn remembered(ctx: &Ctx, operation: &Operation) -> Result<Option<Remembered>> {
    let seen: Option<i64> = sqlx::query_scalar!(
        r#"SELECT 1 as "seen: i64" FROM applied_operations WHERE id = ?1"#,
        operation.id
    )
    .fetch_optional(&ctx.db)
    .await
    .map_err(crate::models::Error::from)?;

    if seen.is_none() {
        return Ok(None);
    }

    // Looked up rather than stored: what the device wants back is the row as it stands,
    // and storing a snapshot would hand it something that was true once.
    // A remade list is answered with the list, for the same reason a re-added item is
    // answered with the item: the device still does not know what the server calls it.
    if let What::MakeList { .. } = &operation.what {
        let list = list::List::get(&ctx.db, list::Lookup::Uuid(operation.list.clone()))
            .await
            .ok();
        return Ok(Some(Remembered { item: None, list }));
    }

    let named = match &operation.what {
        What::Add { item, .. }
        | What::SetDone { item, .. }
        | What::Update { item, .. }
        | What::Delete { item, .. }
        | What::Tag { item, .. } => Some(item.clone()),
        What::MakeList { .. } | What::ClearDone { .. } | What::SetTagOrder { .. } => None,
    };

    match named {
        Some(uuid) => Ok(Some(Remembered { item: find(ctx, &uuid).await?, list: None })),
        None => Ok(Some(Remembered { item: None, list: None })),
    }
}

/// What a resend is answered with: whichever of the two the operation produced.
struct Remembered {
    item: Option<Item>,
    list: Option<List>,
}

async fn remember(ctx: &Ctx, operation: &Operation, who: &user::User) -> Result<()> {
    let kind = operation.what.kind();
    sqlx::query!(
        r#"INSERT OR IGNORE INTO applied_operations (id, user_id, kind) VALUES (?1, ?2, ?3)"#,
        operation.id,
        who.id,
        kind
    )
    .execute(&ctx.db)
    .await
    .map_err(crate::models::Error::from)?;
    Ok(())
}
