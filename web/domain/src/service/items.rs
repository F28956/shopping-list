//! Items are reached through their list, so every check here is a check on the list.
//!
//! An item id is not a capability: holding one gained from somewhere else gets the
//! same `NotFound` as an id that never existed, because the list is what is consulted.

use crate::models::history::Entry;
use crate::models::item::{self, Amount, Item, Name};
use crate::models::list::Role;
use crate::models::user::User;
use crate::models::{OffsetPage, OrderBy, Paging, list, tag, unit};

use super::{Actor, Ctx, Result, ServiceError, lists};
use crate::fuzzy;
use crate::history_rank::{self, Candidate};
use crate::quick_add;

/// Loads an item on a list the actor owns, or reports it missing.
/// Loads an item on a list the actor may act on at `need`, or reports it missing.
///
/// The item's own row says nothing about who may touch it; its list does. A refusal
/// from the list is passed through unchanged, so a viewer editing gets `Forbidden`
/// and a stranger gets `NotFound`.
pub(super) async fn accessible(ctx: &Ctx, actor: &User, id: item::Id, need: Role) -> Result<Item> {
    let item = Item::get(&ctx.db, item::Lookup::Id(id)).await?;
    lists::accessible(ctx, actor, item.list_id, need).await?;
    Ok(item)
}

/// The commonest case: may this person change this item?
pub(super) async fn editable(ctx: &Ctx, actor: &User, id: item::Id) -> Result<Item> {
    accessible(ctx, actor, id, Role::Editor).await
}

/// Adds an item, and remembers that this person buys it.
///
/// Every route in, structured or parsed, comes through here, so anything a person
/// adds is learned — an item added from the API is as much a habit as one typed into
/// the box.
/// `uuid` is what the device called this before the server had heard of it.
///
/// `None` is the online path, where the row is born here and is named here. `Some` is
/// an add that was made with no signal: the device named it at the moment somebody
/// typed it, and everything queued behind it on that device says the same name. Giving
/// the row a different one would orphan all of it.
///
/// A `uuid` the server has already seen is a resend, and returns the row unchanged.
/// That is a stronger promise than the name-matching below, which answers "is this
/// already on the list" -- this answers "have I already applied this exact add", and
/// it holds even after somebody renamed the row in between.
pub async fn create(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    uuid: Option<item::Uuid>,
    name: Name,
    amount: Amount,
    unit_id: Option<unit::Id>,
) -> Result<Item> {
    let owner = actor.person()?;
    lists::editable(ctx, owner, list_id).await?;

    if let Some(named) = uuid.clone()
        && let Ok(already) = Item::get(&ctx.db, item::Lookup::Uuid(named)).await
    {
        // Somebody else's row with a guessed uuid is not this caller's to be handed.
        // The list is the check that matters: they may already edit this one.
        if already.list_id == list_id {
            return Ok(already);
        }
        return Err(ServiceError::InvalidInput);
    }

    // Checked here rather than left to `CHECK (amount > 0)`, so a nonsense amount is
    // the caller's mistake rather than a constraint violation surfacing as a database
    // error -- and so it is refused whether or not the item turns out to exist
    // already, which is the path that does no insert at all.
    if !amount.0.is_finite() || amount.0 <= 0.0 {
        return Err(ServiceError::InvalidInput);
    }

    let unit_id = measured_or_counted(ctx, unit_id).await;

    // Adding something the list already wants changes nothing: it is already there,
    // and that is the whole answer. Two rows saying `Milk` are never two intentions,
    // and neither is `4 kg` when two people each asked for two -- somebody adding a
    // thing has not looked at the amount and is not asking for it to move.
    //
    // Crossed off is the exception, and not really an exception: adding something you
    // have already ticked off is how you say you need it after all, so it comes back
    // -- with the amount it had, untouched.
    //
    // Being idempotent is what makes this safe to replay. An event that says "put
    // milk on the list" can arrive twice, or an hour late, and mean the same thing
    // both times -- see docs/offline.md.
    let item = match Item::alike(&ctx.db, list_id, &name, unit_id).await? {
        Some(existing) if existing.done_at.is_some() => {
            Item::put_back(&ctx.db, existing.id).await?
        }
        Some(existing) => existing,
        // Named here, because nothing named it earlier. When the add came over sync
        // the device had already minted one and it travels with the operation; this
        // is the online path, where the row is born on the server.
        None => {
            let named = uuid.unwrap_or_else(item::Uuid::mint);
            Item::create(&ctx.db, named, list_id, name, amount, unit_id).await?
        }
    };

    Entry::record(&ctx.db, list_id, &item.name, unit_id, Some(item.amount)).await?;
    Entry::prune(&ctx.db, list_id).await?;

    ctx.changes.announce(list_id);
    Ok(item)
}

/// The unit an item ends up with, given what a caller asked for.
///
/// Counted rather than measured is still a unit, and `unit` is the one that says so.
/// Left as NULL, "milk" and "1 unit milk" are different units and so different rows,
/// and the list grows a near-duplicate that nothing will ever merge.
///
/// Applied on every write, not only on the first. It used to live inside `create`
/// alone, so an item added measured and then edited back to nothing kept the NULL --
/// and became exactly the near-duplicate this exists to prevent.
///
/// `unit` missing from the units table is not an error. It is seeded by migration, so
/// its absence means somebody has deliberately taken it out, and refusing every write
/// on a shopping list over that would be the wrong size of reaction.
async fn measured_or_counted(ctx: &Ctx, asked: Option<unit::Id>) -> Option<unit::Id> {
    // The precedence lives in `parsing::add::unit_for`, which the clients call too.
    // Here there is nothing spelled and nothing remembered -- the caller has already
    // resolved both -- so this is only its last step, and it is that step by calling
    // it rather than by repeating it.
    if asked.is_some() {
        return asked;
    }
    let units = unit::Unit::list(&ctx.db, super::everything(), super::by_name())
        .await
        .map(|page| page.items)
        .unwrap_or_default();
    let shared: Vec<parsing::add::Unit> = units
        .iter()
        .map(|u| parsing::add::Unit { id: u.id.0, name: u.name.0.clone(), bare: u.bare })
        .collect();

    parsing::add::unit_for(None, None, &shared).map(unit::Id)
}

/// One page of a list's items, if the list is the actor's.
pub async fn for_list(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    page: Paging,
    order_by: OrderBy<item::Field>,
) -> Result<OffsetPage<Item>> {
    let owner = actor.person()?;
    lists::readable(ctx, owner, list_id).await?;
    Ok(Item::for_list(&ctx.db, list_id, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, actor: &Actor, id: item::Id) -> Result<Item> {
    accessible(ctx, actor.person()?, id, Role::Viewer).await
}

/// Corrects an item.
///
/// Renaming one onto another's name leaves two rows, deliberately: `create` merges
/// because adding twice is one intention entered twice, and an edit is somebody
/// saying what this particular row is. Silently folding it into another would delete
/// a row they did not ask to lose.
/// The row as the device last saw it, when an edit was made against a stale copy.
///
/// Only a client replaying a queue sends this. Somebody editing a row that is on their
/// screen right now has nothing to compare against and needs nothing compared.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub name: Name,
    pub amount: Amount,
    pub unit_id: Option<unit::Id>,
}

/// Replaces what a person typed: name, amount, unit.
///
/// `seen` is what the row looked like when the edit was made, and it is what decides
/// between renaming a row and splitting one. See scenario 5 of `docs/offline.md`:
///
/// * Nothing else touched the row since — a plain edit, in place.
/// * Somebody else edited it, and this edit **changes the name** — their row keeps
///   their edit, and the new name becomes a second row carrying what this device saw.
///   Neither person loses anything they typed.
/// * Somebody else edited it and this edit does *not* change the name — latest wins,
///   in place. Two people arguing about one number is one argument, and two rows both
///   called `Milk` would be a worse answer than either of them winning.
pub async fn update(
    ctx: &Ctx,
    actor: &Actor,
    id: item::Id,
    name: Name,
    amount: Amount,
    unit_id: Option<unit::Id>,
    seen: Option<Seen>,
) -> Result<Item> {
    let owner = actor.person()?;
    let current = editable(ctx, owner, id).await?;

    if let Some(seen) = seen
        && splits(&current, &seen, &name)
    {
        return split(ctx, actor, &current, name, seen).await;
    }

    let unit_id = measured_or_counted(ctx, unit_id).await;
    let item = Item::update(&ctx.db, id, name, amount, unit_id).await?;

    // Correcting a name teaches the correction. The typo it replaced stays until it
    // decays out or is forgotten explicitly — nothing here can tell an edit that
    // fixes a spelling from one that changes the item.
    Entry::record(&ctx.db, item.list_id, &item.name, item.unit_id, Some(item.amount)).await?;

    ctx.changes.announce(item.list_id);
    Ok(item)
}

/// Whether an edit made against a stale copy of a row should become a second row.
///
/// Both halves are needed. A row that has not changed is not contested, however late
/// the edit is; and an edit that leaves the name alone has nothing to split *into*.
fn splits(current: &Item, seen: &Seen, wanted: &Name) -> bool {
    let moved = current.name.0.trim() != seen.name.0.trim()
        || current.amount != seen.amount
        || current.unit_id != seen.unit_id;

    // Compared after trimming and capitalising, because that is what the model would
    // have stored -- otherwise `milk` "renamed" to `Milk` splits a row in two.
    let renaming = current.name != wanted.clone().trimmed().capitalised();

    moved && renaming
}

/// Puts the new name on a row of its own, leaving the contested one alone.
///
/// The new row carries the amount and unit **this device saw**, not the ones it has
/// now: it is the row somebody was looking at when they renamed it, and handing them
/// the other person's number would be giving them something they never asked for.
///
/// The tags come from the original as it stands. A rename is not a re-filing, and a
/// row that lands in no category is a worse answer than one filed where the thing it
/// was renamed from is filed.
async fn split(ctx: &Ctx, actor: &Actor, current: &Item, name: Name, seen: Seen) -> Result<Item> {
    let renamed = create(
        ctx,
        actor,
        current.list_id,
        None,
        name,
        seen.amount,
        seen.unit_id,
    )
    .await?;

    for filed in tag::Tag::for_item(&ctx.db, current.id).await? {
        // Best effort: a tag that will not attach is a row filed less well, which is
        // not worth failing a rename over.
        let _ = tag::Tag::attach(&ctx.db, renamed.id, filed.id).await;
    }

    ctx.changes.announce(current.list_id);
    Ok(renamed)
}

/// Ticks an item off, or puts it back.
pub async fn set_done(ctx: &Ctx, actor: &Actor, id: item::Id, done: bool) -> Result<Item> {
    set_done_at(ctx, actor, id, done, time::OffsetDateTime::now_utc()).await
}

/// Ticks an item off as of a particular moment, for a tick that is being replayed.
///
/// See [`Item::set_done_at`]. The moment has already been clamped by the caller: this
/// layer decides who may tick, not whether a clock is telling the truth.
pub async fn set_done_at(
    ctx: &Ctx,
    actor: &Actor,
    id: item::Id,
    done: bool,
    at: time::OffsetDateTime,
) -> Result<Item> {
    editable(ctx, actor.person()?, id).await?;
    let item = Item::set_done_at(&ctx.db, id, done, at).await?;
    ctx.changes.announce(item.list_id);
    Ok(item)
}

/// Adds an item from one typed line, filling in what this person's history knows.
///
/// The whole of "quick add" lives here rather than in a transport, so the browser and
/// the API cannot drift on what `2 kg apples` means:
///
/// 1. parse the line against the known units,
/// 2. fall back to the remembered unit when the line did not give one,
/// 3. create the item — which records the use,
/// 4. apply the remembered category, so a re-added item files itself.
///
/// Steps 2 and 4 are the difference between a history that only autocompletes and one
/// that pays back: `milk` arrives in pints, under dairy, having typed four letters.
pub async fn quick_add(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    uuid: Option<item::Uuid>,
    line: &str,
) -> Result<Item> {
    let owner = actor.person()?;
    lists::editable(ctx, owner, list_id).await?;

    let units = unit::Unit::list(&ctx.db, super::everything(), super::by_name()).await?;
    let names: Vec<String> = units.items.iter().map(|u| u.name.0.clone()).collect();

    let standalone: Vec<String> = units
        .items
        .iter()
        .filter(|u| u.bare)
        .map(|u| u.name.0.clone())
        .collect();
    // `pint milk` is one pint of milk. Only some units may be read that way -- see
    // `parsing::quick_add::parse_with`.
    let parsed = quick_add::parse_with(line, &names, &standalone);
    let name = Name(parsed.name.clone());

    // What the line said, if it said anything.
    let remembered = Entry::get(&ctx.db, list_id, &name).await?;

    // Which unit this ends up in, by the rule every client also runs -- the line, then
    // what this name was last bought in, then `unit`. See `parsing::add::unit_for`.
    let shared: Vec<parsing::add::Unit> = units
        .items
        .iter()
        .map(|u| parsing::add::Unit { id: u.id.0, name: u.name.0.clone(), bare: u.bare })
        .collect();
    let held = remembered.as_ref().map(|e| parsing::add::Remembered {
        name: e.name.0.clone(),
        unit_id: e.unit_id.map(|u| u.0),
        amount: e.amount.map(|a| a.0),
        tag_ids: Vec::new(),
    });
    let unit_id = parsing::add::unit_for(parsed.unit.as_deref(), held.as_ref(), &shared)
        .map(unit::Id);
    // Both by the shared rule, so a phone deciding this for itself decides it the same
    // way -- see `parsing::add`.
    let amount = Amount(parsing::add::amount_for(&parsed, held.as_ref()));

    // Through `create` rather than the model, so there is one place that learns.
    let item = create(
        ctx,
        actor,
        list_id,
        uuid,
        name.clone(),
        amount,
        unit_id,
    )
    .await?;

    // File it where this person filed it last time -- under everything it was filed
    // under, not just one of them.
    for tag_id in Entry::tags_for(&ctx.db, list_id, &name).await.unwrap_or_default() {
        // A tag deleted since it was remembered is not the caller's problem: the item
        // is already added, and an unfiled item beats a failed add.
        let _ = tag::Tag::attach(&ctx.db, item.id, tag_id).await;
    }

    Ok(item)
}

/// The whole of what this list remembers, for a device to keep.
///
/// `suggestions` answers "what should I offer for these letters" and returns names.
/// This answers "what does this list know", and returns everything a client needs to
/// resolve a line for itself: the unit, how much, and where it gets filed.
///
/// **Why a device needs it at all.** The clients run the same rules the server does —
/// see `parsing::add` — but a rule is only as good as what it is given. Without the
/// history a phone resolving `apples` locally would reach a different answer from the
/// server, which has one; with it, both have the same inputs and cannot disagree.
/// Syncing it is also what makes the memory the household's rather than each device's:
/// what one person taught the list, everybody's phone knows.
pub async fn remembered(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    limit: i64,
) -> Result<Vec<Remembered>> {
    lists::readable(ctx, actor.person()?, list_id).await?;

    let entries = Entry::for_list(&ctx.db, list_id, limit.clamp(0, super::PAGE_MAX)).await?;

    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        // Per entry rather than one join: the list is bounded by `PAGE_MAX` and this
        // reuses the ordering `tags_for` already applies, which is the shop's.
        let tags = Entry::tags_for(&ctx.db, list_id, &entry.name)
            .await
            .unwrap_or_default();
        out.push(Remembered { entry, tags });
    }
    Ok(out)
}

/// One remembered line, with everything a client needs to act on it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Remembered {
    #[serde(flatten)]
    pub entry: Entry,
    pub tags: Vec<crate::models::tag::Id>,
}

/// Forgets one remembered item — the way back from a typo.
///
/// An editor's to do: the memory is shared, so forgetting affects everyone on the
/// list, which is the same standing as putting something on it.
pub async fn forget(ctx: &Ctx, actor: &Actor, list_id: list::Id, name: Name) -> Result<()> {
    lists::editable(ctx, actor.person()?, list_id).await?;
    Entry::forget(&ctx.db, list_id, &name).await?;
    // The memory is part of the list, and a suggestion box showing a typo somebody
    // has just forgotten is the same staleness as a row that has gone.
    ctx.changes.announce(list_id);
    Ok(())
}

/// How many suggestions a caller is offered.
///
/// Here rather than in each transport: the browser was showing every match and the
/// phone the first six, which is two answers to one question. Six is what fits under
/// a field on a phone without covering the list behind it.
/// How many to offer. The number lives with the policy -- see
/// `parsing::suggest::LIMIT`.
pub const SUGGESTIONS: usize = parsing::suggest::LIMIT;

/// What gets bought on this list, for a quick-add suggestion list.
///
/// The list's memory, not the actor's: everyone sharing it sees and feeds the same
/// one, which is the point of keying history on the list.
///
/// `query` is what has been typed so far, matched loosely — see [`crate::fuzzy`]. The
/// matching happens here rather than in a transport so the phone and the browser
/// cannot offer different suggestions for the same letters. `None` is the whole list,
/// in rank order.
pub async fn suggestions(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    limit: i64,
    query: Option<&str>,
) -> Result<Vec<Name>> {
    lists::readable(ctx, actor.person()?, list_id).await?;

    // Read by recency; what to offer and in what order is `parsing::suggest`, which
    // the clients run too. It was written out here and again on the phone, and the two
    // disagreed: this sorted by how well a name matched, that one by how often it is
    // bought, so `mil` offered `milk` here and `milk chocolate` there.
    let entries = Entry::for_list(&ctx.db, list_id, limit.clamp(0, super::PAGE_MAX)).await?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();

    Ok(parsing::suggest::offer(
        query.unwrap_or_default(),
        entries
            .into_iter()
            .map(|e| parsing::suggest::Remembered {
                // Offered in the spelling last used, not the normalised key.
                name: e.display.0,
                uses: e.uses.0,
                last_used_at: e.last_used_at.0.unix_timestamp(),
            })
            .collect(),
        now,
    )
    .into_iter()
    .map(Name)
    .collect())
}


/// Clears everything ticked off one of the actor's lists, returning how many went.
/// Empties the trolley.
///
/// `only` is the dangerous half of this operation made safe. "Clear everything that is
/// done" is a sentence whose meaning changes with the minute it is read: queued on a
/// phone in a shop and replayed an hour later, it would also sweep away the four
/// things somebody at home ticked off meanwhile, which nobody asked for.
///
/// So a client that is replaying says **which rows it meant** at the time, and gets
/// those and nothing else. Rows already gone are not an error -- somebody deleting one
/// of them first is the same outcome by another route. `None` keeps the live meaning,
/// for the button being pressed right now with the list on screen.
pub async fn clear_done(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    only: Option<&[item::Id]>,
) -> Result<u64> {
    let owner = actor.person()?;
    lists::editable(ctx, owner, list_id).await?;
    let cleared = match only {
        Some(ids) => Item::delete_done_among(&ctx.db, list_id, ids).await?,
        None => Item::delete_done(&ctx.db, list_id).await?,
    };
    // Announced even when nothing was ticked off: a watcher that re-reads an
    // unchanged list is harmless, and deciding not to tell it is a second rule that
    // can be wrong.
    ctx.changes.announce(list_id);
    Ok(cleared)
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: item::Id) -> Result<()> {
    // The row is read before it goes, because afterwards there is nothing to say
    // which list to tell.
    let item = editable(ctx, actor.person()?, id).await?;
    Item::delete(&ctx.db, id).await?;
    ctx.changes.announce(item.list_id);
    Ok(())
}
