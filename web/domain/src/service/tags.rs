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
/// The tags of this list, in the order that decides where its items sit.
///
/// Every tag comes back, always, so a caller can group by position in this list and
/// nothing else. Resolved in three steps:
///
/// 1. the order this person set on this list,
/// 2. failing that, the earliest order anyone set here — a list shared with somebody
///    who never opens the settings still walks the route the other person chose,
/// 3. and whatever is left keeps the global order, behind the tags that were placed.
///
/// A list nobody has configured therefore comes back exactly as it does today.
pub async fn order_for(ctx: &Ctx, actor: &Actor, list_id: list::Id) -> Result<Vec<Tag>> {
    let user = actor.person()?;
    lists::readable(ctx, user, list_id).await?;

    let mut chosen = tag::Order::of(&ctx.db, list_id, user.id).await?;
    if chosen.is_empty() {
        chosen = tag::Order::inherited(&ctx.db, list_id).await?;
    }

    let all = Tag::list(&ctx.db, super::everything(), super::by_shop()).await?.items;

    // Placed first, in the order they were placed; then everything else, in the order
    // it already had. A tag that has been deleted since it was placed simply is not
    // in `all`, and drops out here rather than becoming a hole.
    let mut ordered: Vec<Tag> = chosen
        .iter()
        .filter_map(|id| all.iter().find(|t| t.id == *id).cloned())
        .collect();
    ordered.extend(all.into_iter().filter(|t| !chosen.contains(&t.id)));

    Ok(ordered)
}

/// Replaces this person's order on this list. An empty list clears it, putting them
/// back on whatever they would have inherited.
///
/// A viewer may set one: it changes how they see the list and nothing about the list
/// itself, and telling somebody they may read a list but not decide what order they
/// read it in would be a strange kind of permission.
pub async fn set_order(
    ctx: &Ctx,
    actor: &Actor,
    list_id: list::Id,
    tags: &[tag::Id],
) -> Result<()> {
    let user = actor.person()?;
    lists::readable(ctx, user, list_id).await?;

    // Checked rather than trusted: a position given to a tag that does not exist
    // would be a row the resolver silently drops, and a caller that mistyped an id
    // deserves to be told rather than to watch nothing happen.
    for id in tags {
        Tag::get(&ctx.db, tag::Lookup::Id(*id)).await?;
    }

    Ok(tag::Order::set(&ctx.db, list_id, user.id, tags).await?)
}

/// Teaches the memory what this item is now filed under.
///
/// The whole set is read back and stored, rather than the one tag that just moved.
/// Remembering a single tag was the bug: attaching a second overwrote the first, so
/// an item filed under a shop and a category came back with only whichever was last.
async fn remember(ctx: &Ctx, item: &item::Item) -> Result<()> {
    let held = Tag::for_item(&ctx.db, item.id).await?;
    let ids: Vec<tag::Id> = held.iter().map(|t| t.id).collect();
    Entry::remember_tags(&ctx.db, item.list_id, &item.name, &ids).await?;
    Ok(())
}

pub async fn attach(ctx: &Ctx, actor: &Actor, item_id: item::Id, tag_id: tag::Id) -> Result<()> {
    let owner = actor.person()?;
    let item = items::editable(ctx, owner, item_id).await?;
    Tag::attach(&ctx.db, item_id, tag_id).await?;
    ctx.changes.announce(item.list_id);

    // Filing something is the strongest signal about where it belongs, so the next
    // time it is added it arrives already filed. Best-effort: an item that has never
    // been through quick-add has no history row, and that is not a failure to tag.
    let _ = remember(ctx, &item).await;

    Ok(())
}

pub async fn detach(ctx: &Ctx, actor: &Actor, item_id: item::Id, tag_id: tag::Id) -> Result<()> {
    let owner = actor.person()?;
    let item = items::editable(ctx, owner, item_id).await?;
    Tag::detach(&ctx.db, item_id, tag_id).await?;
    ctx.changes.announce(item.list_id);

    // Unfiling is a signal too: stop putting it there.
    let _ = remember(ctx, &item).await;

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
