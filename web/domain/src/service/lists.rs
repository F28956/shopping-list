//! Lists belong to their owner. Sharing is not implemented, so "can see" and "owns"
//! are the same question here — when `list_members` grows teeth, [`owned`] is the one
//! place that has to learn the difference.

use crate::models::invite::{Invite, Token};
use crate::models::list::{self, List, ListMember, Name, Role};
use crate::models::user;
use crate::models::user::User;
use crate::models::{OffsetPage, OrderBy, Paging};

use super::{Actor, Ctx, Result, ServiceError};

/// Loads a list the actor may act on at `need`, or explains why not.
///
/// The one door. Every list, item, tag and history operation comes through here, so
/// sharing is a change to this function rather than to nine call sites.
///
/// Two different refusals, and the difference matters:
///
/// * someone with no access at all gets `NotFound` — a guessed id must not confirm
///   that a list exists;
/// * a member whose role is too low gets `Forbidden` — they can already see it, so
///   pretending otherwise is a lie that reads as a bug.
pub(super) async fn accessible(ctx: &Ctx, actor: &User, id: list::Id, need: Role) -> Result<List> {
    let list = List::get(&ctx.db, list::Lookup::Id(id)).await?;

    match ListMember::role_of(&ctx.db, id, actor.id).await? {
        // Roles are ordered, so "enough" is just a comparison.
        Some(held) if held >= need => Ok(list),
        Some(_) => Err(ServiceError::refused("list", actor)),
        None => Err(ServiceError::hidden("list", actor)),
    }
}

/// Shorthand for the commonest check: may this person change what is on the list?
pub(super) async fn editable(ctx: &Ctx, actor: &User, id: list::Id) -> Result<List> {
    accessible(ctx, actor, id, Role::Editor).await
}

/// Shorthand for reading.
pub(super) async fn readable(ctx: &Ctx, actor: &User, id: list::Id) -> Result<List> {
    accessible(ctx, actor, id, Role::Viewer).await
}

pub async fn create(ctx: &Ctx, actor: &Actor, name: Name) -> Result<List> {
    let owner = actor.person()?;
    let list = List::create(&ctx.db, list::Uuid::mint(), owner.id, name).await?;

    // Told to the person, not to the list: a list that has just been made has no
    // watchers, so announcing it to itself reaches nobody -- which is why one made on
    // a phone never turned up on a Mac left open beside it.
    ctx.changes.announce_lists_of(owner.id);
    Ok(list)
}

/// One page of the lists this person can see — the ones they own and the ones they
/// have been given. The person comes from the actor, so a caller cannot ask for
/// anybody else's.
pub async fn for_user(
    ctx: &Ctx,
    actor: &Actor,
    page: Paging,
    order_by: OrderBy<list::Field>,
) -> Result<OffsetPage<List>> {
    let owner = actor.person()?;
    Ok(List::visible_to(&ctx.db, owner.id, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<List> {
    readable(ctx, actor.person()?, id).await
}

/// Renaming is the owner's: a list's name is how everyone else finds it.
pub async fn update(ctx: &Ctx, actor: &Actor, id: list::Id, name: Name) -> Result<List> {
    accessible(ctx, actor.person()?, id, Role::Owner).await?;
    let list = List::update(&ctx.db, id, name).await?;
    ctx.changes.announce(id);
    // Its name is on everybody's list of lists, not only on the list itself.
    tell_everyone_on(ctx, id).await;
    Ok(list)
}

/// Deletes a list and everything on it — `items` cascades. A list with items on it is
/// therefore never `InUse`, unlike a unit some item still points at.
pub async fn delete(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<()> {
    accessible(ctx, actor.person()?, id, Role::Owner).await?;
    // Read before it goes: afterwards there is nobody left to tell.
    let audience = people_ids(ctx, id).await;

    List::delete(&ctx.db, id).await?;
    // Watchers are told last, so that anyone re-reading finds it already gone rather
    // than racing the delete and being told it is still there.
    ctx.changes.announce(id);
    for who in audience {
        ctx.changes.announce_lists_of(who);
    }
    Ok(())
}

// ------------------------------------------------------------------- sharing

/// Creates an invitation and returns the token to put in a link.
///
/// The token is returned once and never again: only its hash is stored, so an owner
/// who loses the link makes another rather than looking the old one up. That is the
/// same trade a password reset makes, for the same reason.
///
/// `Role::Owner` cannot be invited — ownership is not something a link confers.
pub async fn invite(ctx: &Ctx, actor: &Actor, id: list::Id, role: Role) -> Result<Token> {
    let owner = actor.person()?;
    accessible(ctx, owner, id, Role::Owner).await?;

    if role >= Role::Owner {
        return Err(ServiceError::InvalidInput);
    }

    let token = Token(new_token());
    Invite::create(&ctx.db, &token, id, role, owner.id).await?;

    Ok(token)
}

/// 256 bits from the operating system, hex-encoded.
///
/// Guessing has to be hopeless rather than merely hard: this token is the whole of
/// the credential, and it travels in a URL where it may be logged, pasted and
/// forwarded.
fn new_token() -> String {
    use rand::Rng;

    // ThreadRng, which is a cryptographic generator seeded from the operating system
    // — not one of the fast reproducible ones. A predictable token is no token.
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Redeems an invitation, putting the actor on the list.
///
/// Idempotent: following the same link twice is a double-click, not an error. An
/// invitation that would lower an existing role is ignored rather than applied, so a
/// stale editor link cannot demote an owner or another editor.
pub async fn join(ctx: &Ctx, actor: &Actor, token: &Token) -> Result<List> {
    let joiner = actor.person()?;
    let invite = Invite::claim(&ctx.db, token).await?;

    let held = ListMember::role_of(&ctx.db, invite.list_id, joiner.id).await?;

    // A spent link still works for the person who spent it — following it twice is a
    // double-click, and refusing the second is a worse answer than doing nothing. It
    // does not work for anybody else: a link lives for a week, and without this a
    // forwarded message or a screenshot let a stranger join long after the person it
    // was written for already had.
    let owner = List::get(&ctx.db, list::Lookup::Id(invite.list_id))
        .await
        .map(|l| l.owner_id == joiner.id)
        .unwrap_or(false);

    let spent = invite.used_at.is_some();

    if spent && held.is_none() && !owner {
        return Err(ServiceError::NotFound);
    }

    // A spent link grants nothing — not even to somebody already here. Without the
    // `!spent`, a viewer who came by their own link and later got hold of a used
    // editor one would be promoted by it: narrower than admitting a stranger, but
    // still a link doing something after it was spent.
    if !spent && held.is_none_or(|held| held < invite.role) {
        ListMember::put(&ctx.db, invite.list_id, joiner.id, invite.role).await?;
    }
    Invite::mark_used(&ctx.db, token).await?;

    let list = List::get(&ctx.db, list::Lookup::Id(invite.list_id)).await?;

    // Three streams, because three screens are now out of date, and telling only the
    // first is what left a share sheet saying "who can see it: you" while somebody
    // else was already reading the list.
    //
    // Removal has told all three since it was written; joining told one. The pair are
    // the same event in opposite directions and had drifted apart.
    ctx.changes.announce_lists_of(joiner.id); // they have a list they did not have
    ctx.changes.announce_lists_of(list.owner_id); // the owner has one more sharer
    ctx.changes.announce(invite.list_id); // and the list itself has a new reader

    Ok(list)
}

/// What this person may do on this list.
pub async fn role(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<Role> {
    let user = actor.person()?;
    // Through `readable` first, so somebody with no access is told nothing rather
    // than being handed the shape of an answer.
    readable(ctx, user, id).await?;

    ListMember::role_of(&ctx.db, id, user.id)
        .await?
        .ok_or(ServiceError::NotFound)
}

/// How many people each of the actor's lists is shared with, for an index that wants
/// to say so without asking per row.
pub async fn share_counts(ctx: &Ctx, actor: &Actor) -> Result<std::collections::HashMap<i64, i64>> {
    Ok(ListMember::counts_for(&ctx.db, actor.person()?.id).await?)
}

/// Everyone who can see a list: the owner, and every member.
async fn people_ids(ctx: &Ctx, id: list::Id) -> Vec<user::Id> {
    let mut who = Vec::new();

    if let Ok(list) = List::get(&ctx.db, list::Lookup::Id(id)).await {
        who.push(list.owner_id);
    }
    if let Ok(members) = ListMember::for_list(&ctx.db, id).await {
        who.extend(members.into_iter().map(|m| m.user_id));
    }

    who
}

/// Tells everyone who can see a list that their set of lists has changed.
async fn tell_everyone_on(ctx: &Ctx, id: list::Id) {
    for who in people_ids(ctx, id).await {
        ctx.changes.announce_lists_of(who);
    }
}

/// Everyone the list is shared with, and the role each holds.
///
/// Readable by any member: knowing who else can see your shopping is part of knowing
/// what sharing means. The owner is not among them — see [`ListMember`].
pub async fn members(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<Vec<ListMember>> {
    readable(ctx, actor.person()?, id).await?;
    Ok(ListMember::for_list(&ctx.db, id).await?)
}

/// Everyone on a list, as a person rather than an id.
///
/// The owner comes first and is included — [`members`] leaves them out, because
/// membership is a row in `list_members` and the owner has none, but "who can see
/// this list" plainly means them too.
///
/// Names and addresses are shown to other members and nowhere else. Somebody invited
/// to a shared list already knows who they are sharing it with; not saying so leaves
/// a screen full of "Someone", which tells you a list is shared and nothing about who
/// with.
pub async fn people_on(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<Vec<Person>> {
    let asking = actor.person()?;
    let list = readable(ctx, asking, id).await?;

    let mut people = Vec::new();

    if let Ok(owner) = user::User::get(&ctx.db, user::Lookup::Id(list.owner_id)).await {
        people.push(Person {
            user: owner,
            role: Role::Owner,
        });
    }

    for member in ListMember::for_list(&ctx.db, id).await? {
        if let Ok(user) = user::User::get(&ctx.db, user::Lookup::Id(member.user_id)).await {
            people.push(Person {
                user,
                role: member.role,
            });
        }
    }

    Ok(people)
}

/// Somebody who can see a list, and what they may do with it.
#[derive(Debug, Clone)]
pub struct Person {
    pub user: user::User,
    pub role: Role,
}

/// Withdraws every outstanding invitation to a list.
pub async fn revoke_invites(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<u64> {
    accessible(ctx, actor.person()?, id, Role::Owner).await?;
    Ok(Invite::revoke_all(&ctx.db, id).await?)
}

/// Removes someone from a list.
///
/// The owner may remove anybody; anybody may remove themselves. An owner cannot
/// remove themselves at all: there is no transfer, so a list without its owner would
/// be a list nobody could rename or delete. Leaving is deleting, for them.
pub async fn remove_member(ctx: &Ctx, actor: &Actor, id: list::Id, who: user::Id) -> Result<()> {
    let actor_user = actor.person()?;
    let list = readable(ctx, actor_user, id).await?;

    if who == list.owner_id {
        return Err(ServiceError::InvalidInput);
    }
    if who != actor_user.id {
        accessible(ctx, actor_user, id, Role::Owner).await?;
    }

    ListMember::remove(&ctx.db, id, who).await?;

    // The person removed has one list fewer; the owner has one sharer fewer.
    ctx.changes.announce_lists_of(who);
    ctx.changes.announce_lists_of(list.owner_id);
    Ok(())
}
