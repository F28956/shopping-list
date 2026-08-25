//! Items are reached through their list, so every check here is a check on the list.
//!
//! An item id is not a capability: holding one gained from somewhere else gets the
//! same `NotFound` as an id that never existed, because the list is what is consulted.

use crate::models::history::Entry;
use crate::models::item::{self, Amount, Item, Name};
use crate::models::list::Role;
use crate::models::user::User;
use crate::models::{OffsetPage, OrderBy, Paging, list, tag, unit};

use super::{Actor, Ctx, Result, lists};
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
pub async fn create(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    name: Name,
    amount: Amount,
    unit_id: Option<unit::Id>,
) -> Result<Item> {
    let owner = actor.person()?;
    lists::editable(ctx, owner, list_id).await?;

    let item = Item::create(&ctx.db, list_id, name, amount, unit_id).await?;

    Entry::record(&ctx.db, list_id, &item.name, unit_id).await?;
    Entry::prune(&ctx.db, list_id).await?;

    ctx.changes.announce(list_id);
    Ok(item)
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

pub async fn update(
    ctx: &Ctx,
    actor: &Actor,
    id: item::Id,
    name: Name,
    amount: Amount,
    unit_id: Option<unit::Id>,
) -> Result<Item> {
    let owner = actor.person()?;
    editable(ctx, owner, id).await?;
    let item = Item::update(&ctx.db, id, name, amount, unit_id).await?;

    // Correcting a name teaches the correction. The typo it replaced stays until it
    // decays out or is forgotten explicitly — nothing here can tell an edit that
    // fixes a spelling from one that changes the item.
    Entry::record(&ctx.db, item.list_id, &item.name, item.unit_id).await?;

    ctx.changes.announce(item.list_id);
    Ok(item)
}

/// Ticks an item off, or puts it back.
pub async fn set_done(ctx: &Ctx, actor: &Actor, id: item::Id, done: bool) -> Result<Item> {
    editable(ctx, actor.person()?, id).await?;
    let item = Item::set_done(&ctx.db, id, done).await?;
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
pub async fn quick_add(ctx: &Ctx, actor: &Actor, list_id: list::Id, line: &str) -> Result<Item> {
    let owner = actor.person()?;
    lists::editable(ctx, owner, list_id).await?;

    let units = unit::Unit::list(&ctx.db, super::everything(), super::by_name()).await?;
    let names: Vec<String> = units.items.iter().map(|u| u.name.0.clone()).collect();

    let parsed = quick_add::parse(line, &names);
    let name = Name(parsed.name);

    // What the line said, if it said anything.
    let spelled_unit = parsed
        .unit
        .and_then(|u| units.items.iter().find(|x| x.name.0 == u).map(|x| x.id));

    let remembered = Entry::get(&ctx.db, list_id, &name).await?;
    let unit_id = spelled_unit.or_else(|| remembered.as_ref().and_then(|e| e.unit_id));

    // Through `create` rather than the model, so there is one place that learns.
    let item = create(
        ctx,
        actor,
        list_id,
        name.clone(),
        Amount(parsed.amount),
        unit_id,
    )
    .await?;

    // File it where this person filed it last time.
    if let Some(tag_id) = remembered.as_ref().and_then(|e| e.tag_id) {
        // A tag deleted since it was remembered is not the caller's problem: the item
        // is already added, and an unfiled item beats a failed add.
        let _ = tag::Tag::attach(&ctx.db, item.id, tag_id).await;
    }

    Ok(item)
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

    // Read by recency, offer by rank: the query bounds how much is considered, and
    // `history_rank` decides the order — see there for why not in SQL.
    let entries = Entry::for_list(&ctx.db, list_id, limit.clamp(0, super::PAGE_MAX)).await?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();

    let ranked = history_rank::rank(
        entries
            .into_iter()
            .map(|e| Candidate {
                uses: e.uses.0,
                last_used_at: e.last_used_at.0.unix_timestamp(),
                // Offered in the spelling last used, not the normalised key.
                value: e.display.0,
            })
            .collect(),
        now,
    );

    let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) else {
        return Ok(ranked.into_iter().map(Name).collect());
    };

    // Scored, then ranked: how well it matches what was typed decides the order, and
    // how often it is bought breaks the ties. `position` is the rank order, so a
    // stable sort on it keeps the more-used of two equal matches first.
    let mut matches: Vec<(i32, usize, String)> = ranked
        .into_iter()
        .enumerate()
        .filter_map(|(rank, name)| fuzzy::score(query, &name).map(|s| (s, rank, name)))
        .collect();
    matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    Ok(matches.into_iter().map(|(_, _, name)| Name(name)).collect())
}

/// Clears everything ticked off one of the actor's lists, returning how many went.
pub async fn clear_done(ctx: &Ctx, actor: &Actor, list_id: list::Id) -> Result<u64> {
    let owner = actor.person()?;
    lists::editable(ctx, owner, list_id).await?;
    let cleared = Item::delete_done(&ctx.db, list_id).await?;
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
