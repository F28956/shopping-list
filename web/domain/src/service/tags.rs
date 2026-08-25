//! Tags are shared reference data like units — read by anyone, written by the system
//! — but attaching one to an item is a change to that item, so it follows the item's
//! ownership rather than the tag's.

use crate::models::tag::{self, Colour, Emoji, Name, Tag};
use crate::models::{OffsetPage, OrderBy, Paging, item, list};

use super::{Actor, Ctx, Result, ServiceError, items, lists};

pub async fn list(
    ctx: &Ctx,
    actor: &Actor,
    page: Paging,
    order_by: OrderBy<tag::Field>,
) -> Result<OffsetPage<Tag>> {
    readable(actor)?;
    Ok(Tag::list(&ctx.db, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, actor: &Actor, by: tag::Lookup) -> Result<Tag> {
    readable(actor)?;
    Ok(Tag::get(&ctx.db, by).await?)
}

pub async fn create(
    ctx: &Ctx,
    actor: &Actor,
    name: Name,
    colour: Option<Colour>,
    emoji: Option<Emoji>,
) -> Result<Tag> {
    writable(actor)?;
    Ok(Tag::create(&ctx.db, name, colour, emoji).await?)
}

pub async fn update(
    ctx: &Ctx,
    actor: &Actor,
    id: tag::Id,
    name: Name,
    colour: Option<Colour>,
    emoji: Option<Emoji>,
) -> Result<Tag> {
    writable(actor)?;
    Ok(Tag::update(&ctx.db, id, name, colour, emoji).await?)
}

pub async fn delete(ctx: &Ctx, actor: &Actor, id: tag::Id) -> Result<()> {
    writable(actor)?;
    Ok(Tag::delete(&ctx.db, id).await?)
}

/// The tags on one of the actor's items.
pub async fn for_item(ctx: &Ctx, actor: &Actor, item_id: item::Id) -> Result<Vec<Tag>> {
    items::owned(ctx, actor.person()?, item_id).await?;
    Ok(Tag::for_item(&ctx.db, item_id).await?)
}

/// Every tag on every item of one of the actor's lists, grouped by item.
///
/// The list is checked once and the tags fetched in one query, so rendering a list
/// page costs two round trips rather than one per item.
pub async fn on_list(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
) -> Result<std::collections::HashMap<i64, Vec<Tag>>> {
    lists::owned(ctx, actor.person()?, list_id).await?;

    let mut by_item: std::collections::HashMap<i64, Vec<Tag>> = std::collections::HashMap::new();
    for (item_id, tag) in Tag::for_list(&ctx.db, list_id).await? {
        by_item.entry(item_id.0).or_default().push(tag);
    }
    Ok(by_item)
}

/// Tagging is an edit to the item, so it needs the item, not the tag.
pub async fn attach(ctx: &Ctx, actor: &Actor, item_id: item::Id, tag_id: tag::Id) -> Result<()> {
    items::owned(ctx, actor.person()?, item_id).await?;
    Ok(Tag::attach(&ctx.db, item_id, tag_id).await?)
}

pub async fn detach(ctx: &Ctx, actor: &Actor, item_id: item::Id, tag_id: tag::Id) -> Result<()> {
    items::owned(ctx, actor.person()?, item_id).await?;
    Ok(Tag::detach(&ctx.db, item_id, tag_id).await?)
}

fn readable(_actor: &Actor) -> Result<()> {
    Ok(())
}

fn writable(actor: &Actor) -> Result<()> {
    if actor.is_system() {
        return Ok(());
    }
    if let Ok(person) = actor.person() {
        return Err(ServiceError::forbidden("tag (write)", person));
    }
    Err(ServiceError::Unauthenticated)
}
