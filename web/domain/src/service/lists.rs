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
    Ok(List::create(&ctx.db, owner.id, name).await?)
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
    Ok(List::update(&ctx.db, id, name).await?)
}

/// Deletes a list and everything on it — `items` cascades. A list with items on it is
/// therefore never `InUse`, unlike a unit some item still points at.
pub async fn delete(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<()> {
    accessible(ctx, actor.person()?, id, Role::Owner).await?;
    Ok(List::delete(&ctx.db, id).await?)
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
    if held.is_none_or(|held| held < invite.role) {
        ListMember::put(&ctx.db, invite.list_id, joiner.id, invite.role).await?;
    }
    Invite::mark_used(&ctx.db, token).await?;

    Ok(List::get(&ctx.db, list::Lookup::Id(invite.list_id)).await?)
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

/// Everyone the list is shared with, and the role each holds.
///
/// Readable by any member: knowing who else can see your shopping is part of knowing
/// what sharing means. The owner is not among them — see [`ListMember`].
pub async fn members(ctx: &Ctx, actor: &Actor, id: list::Id) -> Result<Vec<ListMember>> {
    readable(ctx, actor.person()?, id).await?;
    Ok(ListMember::for_list(&ctx.db, id).await?)
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

    Ok(ListMember::remove(&ctx.db, id, who).await?)
}
