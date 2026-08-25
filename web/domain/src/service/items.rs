//! Items are reached through their list, so every check here is a check on the list.
//!
//! An item id is not a capability: holding one gained from somewhere else gets the
//! same `NotFound` as an id that never existed, because the list is what is consulted.

use crate::models::history::Entry;
use crate::models::item::{self, Amount, Item, Name};
use crate::models::user::User;
use crate::models::{OffsetPage, OrderBy, Paging, list, tag, unit};

use super::{Actor, Ctx, Result, ServiceError, lists};
use crate::history_rank::{self, Candidate};
use crate::quick_add;

/// Loads an item on a list the actor owns, or reports it missing.
pub(super) async fn owned(ctx: &Ctx, owner: &User, id: item::Id) -> Result<Item> {
    let item = Item::get(&ctx.db, item::Lookup::Id(id)).await?;

    // The item's own row says nothing about who may touch it; its list does.
    match lists::owned(ctx, owner, item.list_id).await {
        Ok(_) => Ok(item),
        Err(ServiceError::NotFound) => Err(ServiceError::forbidden("item", owner)),
        Err(e) => Err(e),
    }
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
    lists::owned(ctx, owner, list_id).await?;

    let item = Item::create(&ctx.db, list_id, name, amount, unit_id).await?;

    Entry::record(&ctx.db, owner.id, &item.name, unit_id).await?;
    Entry::prune(&ctx.db, owner.id).await?;

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
    lists::owned(ctx, owner, list_id).await?;
    Ok(Item::for_list(&ctx.db, list_id, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, actor: &Actor, id: item::Id) -> Result<Item> {
    owned(ctx, actor.person()?, id).await
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
    owned(ctx, owner, id).await?;
    let item = Item::update(&ctx.db, id, name, amount, unit_id).await?;

    // Correcting a name teaches the correction. The typo it replaced stays until it
    // decays out or is forgotten explicitly — nothing here can tell an edit that
    // fixes a spelling from one that changes the item.
    Entry::record(&ctx.db, owner.id, &item.name, item.unit_id).await?;

    Ok(item)
}

/// Ticks an item off, or puts it back.
pub async fn set_done(ctx: &Ctx, actor: &Actor, id: item::Id, done: bool) -> Result<Item> {
    owned(ctx, actor.person()?, id).await?;
    Ok(Item::set_done(&ctx.db, id, done).await?)
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
    lists::owned(ctx, owner, list_id).await?;

    let units = unit::Unit::list(&ctx.db, super::everything(), super::by_name()).await?;
    let names: Vec<String> = units.items.iter().map(|u| u.name.0.clone()).collect();

    let parsed = quick_add::parse(line, &names);
    let name = Name(parsed.name);

    // What the line said, if it said anything.
    let spelled_unit = parsed
        .unit
        .and_then(|u| units.items.iter().find(|x| x.name.0 == u).map(|x| x.id));

    let remembered = Entry::get(&ctx.db, owner.id, &name).await?;
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
pub async fn forget(ctx: &Ctx, actor: &Actor, name: Name) -> Result<()> {
    let owner = actor.person()?;
    Ok(Entry::forget(&ctx.db, owner.id, &name).await?)
}

/// What this person has bought before, for a quick-add suggestion list.
///
/// Their own history only — the owner comes from the actor, so there is no way to ask
/// what somebody else buys.
pub async fn suggestions(ctx: &Ctx, actor: &Actor, limit: i64) -> Result<Vec<Name>> {
    let owner = actor.person()?;

    // Read by recency, offer by rank: the query bounds how much is considered, and
    // `history_rank` decides the order — see there for why not in SQL.
    let entries = Entry::for_user(&ctx.db, owner.id, limit.clamp(0, 500)).await?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();

    let ranked = history_rank::rank(
        entries
            .into_iter()
            .map(|e| Candidate {
                uses: e.uses.0,
                last_used_at: e.last_used_at.0.unix_timestamp(),
                // Offered in the spelling they last used, not the normalised key.
                value: e.display.0,
            })
            .collect(),
        now,
    );

    Ok(ranked.into_iter().map(Name).collect())
}

/// Clears everything ticked off one of the actor's lists, returning how many went.
pub async fn clear_done(ctx: &Ctx, actor: &Actor, list_id: list::Id) -> Result<u64> {
    let owner = actor.person()?;
    lists::owned(ctx, owner, list_id).await?;
    Ok(Item::delete_done(&ctx.db, list_id).await?)
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: item::Id) -> Result<()> {
    owned(ctx, actor.person()?, id).await?;
    Ok(Item::delete(&ctx.db, id).await?)
}
