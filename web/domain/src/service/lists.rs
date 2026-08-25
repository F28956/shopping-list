//! Lists belong to their owner. Sharing is not implemented, so "can see" and "owns"
//! are the same question here — when `list_members` grows teeth, [`owned`] is the one
//! place that has to learn the difference.

use crate::models::list::{self, List, Name};
use crate::models::user::User;
use crate::models::{OffsetPage, OrderBy, Paging};

use super::{Actor, Ctx, Result, ServiceError};

/// Loads a list the actor owns, or reports it missing.
///
/// Shared by every operation here and by [`super::items`], so the ownership rule
/// exists once. A list the actor does not own reads as `NotFound`, never `Forbidden`.
pub(super) async fn owned(ctx: &Ctx, owner: &User, id: list::Id) -> Result<List> {
    let list = List::get(&ctx.db, list::Lookup::Id(id)).await?;

    if list.owner_id != owner.id {
        return Err(ServiceError::forbidden("list", owner));
    }

    Ok(list)
}

pub async fn create(ctx: &Ctx, actor: &Actor, name: Name) -> Result<List> {
    let owner = actor.person()?;
    Ok(List::create(&ctx.db, owner.id, name).await?)
}

/// One page of the actor's own lists. The owner comes from the actor, so a caller
/// cannot ask for anybody else's.
pub async fn for_user(
    ctx: &Ctx,
    actor: &Actor,
    page: Paging,
    order_by: OrderBy<list::Field>,
) -> Result<OffsetPage<List>> {
    let owner = actor.person()?;
    Ok(List::for_user(&ctx.db, owner.id, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<List> {
    owned(ctx, actor.person()?, id).await
}

pub async fn update(ctx: &Ctx, actor: &Actor, id: list::Id, name: Name) -> Result<List> {
    owned(ctx, actor.person()?, id).await?;
    Ok(List::update(&ctx.db, id, name).await?)
}

/// Deletes a list and everything on it — `items` cascades. A list with items on it is
/// therefore never `InUse`, unlike a unit some item still points at.
pub async fn delete(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<()> {
    owned(ctx, actor.person()?, id).await?;
    Ok(List::delete(&ctx.db, id).await?)
}
