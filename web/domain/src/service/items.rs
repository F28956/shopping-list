//! Items are reached through their list, so every check here is a check on the list.
//!
//! An item id is not a capability: holding one gained from somewhere else gets the
//! same `NotFound` as an id that never existed, because the list is what is consulted.

use crate::models::item::{self, Amount, Item, Name};
use crate::models::user::User;
use crate::models::{OffsetPage, OrderBy, Paging, list, unit};

use super::{Actor, Ctx, Result, ServiceError, lists};

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
    Ok(Item::create(&ctx.db, list_id, name, amount, unit_id).await?)
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
    owned(ctx, actor.person()?, id).await?;
    Ok(Item::update(&ctx.db, id, name, amount, unit_id).await?)
}

/// Ticks an item off, or puts it back.
pub async fn set_done(ctx: &Ctx, actor: &Actor, id: item::Id, done: bool) -> Result<Item> {
    owned(ctx, actor.person()?, id).await?;
    Ok(Item::set_done(&ctx.db, id, done).await?)
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: item::Id) -> Result<()> {
    owned(ctx, actor.person()?, id).await?;
    Ok(Item::delete(&ctx.db, id).await?)
}
