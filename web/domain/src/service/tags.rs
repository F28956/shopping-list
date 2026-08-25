//! Tags are shared reference data like units — read by anyone, written by the system
//! — but attaching one to an item is a change to that item, so it follows the item's
//! ownership rather than the tag's.

use crate::models::history::Entry;
use crate::models::tag::{self, Colour, Emoji, Name, Tag};
use crate::models::{OffsetPage, OrderBy, Paging, item, list};

use super::{Actor, Ctx, Result, ServiceError, items, lists};

/// Readable by any actor: there is no owner to check against, and a unit or a tag
/// tells you nothing about anybody. The `actor` argument stays for the signature all
/// service calls share, and because a future rule would land here.
pub async fn list(
    ctx: &Ctx,
    _actor: &Actor,
    page: Paging,
    order_by: OrderBy<tag::Field>,
) -> Result<OffsetPage<Tag>> {
    Ok(Tag::list(&ctx.db, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, _actor: &Actor, by: tag::Lookup) -> Result<Tag> {
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
    items::accessible(
        ctx,
        actor.person()?,
        item_id,
        crate::models::list::Role::Viewer,
    )
    .await?;
    Ok(Tag::for_item(&ctx.db, item_id).await?)
}

/// Every tag on every item of one of the actor's lists, grouped by item.
///
/// The list is checked once and the tags fetched in one query, so rendering a list
/// page costs two round trips rather than one per item.
pub async fn for_list(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
) -> Result<std::collections::HashMap<i64, Vec<Tag>>> {
    lists::readable(ctx, actor.person()?, list_id).await?;

    let mut by_item: std::collections::HashMap<i64, Vec<Tag>> = std::collections::HashMap::new();
    for (item_id, tag) in Tag::for_list(&ctx.db, list_id).await? {
        by_item.entry(item_id.0).or_default().push(tag);
    }
    Ok(by_item)
}

/// Tagging is an edit to the item, so it needs the item, not the tag.
pub async fn attach(ctx: &Ctx, actor: &Actor, item_id: item::Id, tag_id: tag::Id) -> Result<()> {
    let owner = actor.person()?;
    let item = items::editable(ctx, owner, item_id).await?;
    Tag::attach(&ctx.db, item_id, tag_id).await?;
    ctx.changes.announce(item.list_id);

    // Filing something is the strongest signal about where it belongs, so the next
    // time it is added it arrives already filed. Best-effort: an item that has never
    // been through quick-add has no history row, and that is not a failure to tag.
    let _ = Entry::remember_tag(&ctx.db, item.list_id, &item.name, Some(tag_id)).await;

    Ok(())
}

pub async fn detach(ctx: &Ctx, actor: &Actor, item_id: item::Id, tag_id: tag::Id) -> Result<()> {
    let owner = actor.person()?;
    let item = items::editable(ctx, owner, item_id).await?;
    Tag::detach(&ctx.db, item_id, tag_id).await?;
    ctx.changes.announce(item.list_id);

    // Unfiling is a signal too: stop putting it there.
    if let Ok(Some(entry)) = Entry::get(&ctx.db, item.list_id, &item.name).await
        && entry.tag_id == Some(tag_id)
    {
        let _ = Entry::remember_tag(&ctx.db, item.list_id, &item.name, None).await;
    }

    Ok(())
}

fn writable(actor: &Actor) -> Result<()> {
    if actor.is_system() {
        return Ok(());
    }
    if let Ok(person) = actor.person() {
        return Err(ServiceError::hidden("tag (write)", person));
    }
    Err(ServiceError::Unauthenticated)
}
