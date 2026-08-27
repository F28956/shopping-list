//! A person may read and edit exactly one user: themselves.
//!
//! There is no "get user by id" for people, only [`me`]. Nothing in this application
//! needs one person to look another up, and an endpoint that could would be the first
//! thing to leak an address.

use crate::models::user::{self, Email, Name, User};
use crate::models::{OffsetPage, OrderBy, Paging};

use super::{Actor, Ctx, Result, ServiceError};

/// The signed-in person's own record.
pub async fn me(_ctx: &Ctx, actor: &Actor) -> Result<User> {
    Ok(actor.person()?.clone())
}

/// Replaces the actor's own profile. `None` clears, as in the model.
pub async fn update_profile(
    ctx: &Ctx,
    actor: &Actor,
    name: Option<Name>,
    email: Option<Email>,
) -> Result<User> {
    let me = actor.person()?;
    Ok(User::update(&ctx.db, me.id, name, email).await?)
}

/// Closes the actor's own account.
///
/// Their lists, items and notes go with it — every one of those foreign keys is
/// `ON DELETE CASCADE` — and so does their admitted address, which is the point of
/// closing an account rather than merely signing out.
///
/// Two things are decided here rather than left to the cascade.
///
/// **A shared list is handed over, not deleted.** Cascading would take somebody
/// else's shopping with it: they did nothing, and they would open the app to find it
/// gone. It goes to the longest-standing member, who is the likeliest to have been
/// there before this person and the likeliest to still want it. What is erased is
/// this person — their row, their address, their sessions — and a list several people
/// wrote is not solely theirs to take away. The `Role` note says there is no transfer,
/// and this is the exception it names.
///
/// **The last owner of the server cannot close their account.** It is A5's rule
/// arrived at from a third direction: a server with nobody who can administer it has
/// no way back that does not involve `sqlite3` on the host. They must promote somebody
/// first, and the message says so.
pub async fn close_account(ctx: &Ctx, actor: &Actor) -> Result<()> {
    use crate::models::admission::Admitted;
    use crate::models::list::List;

    let me = actor.person()?;

    if super::admission::is_owner(ctx, me.id).await?
        && crate::models::admission::owner_count(&ctx.db).await? <= 1
    {
        return Err(ServiceError::InUse);
    }

    for list in List::owned_by(&ctx.db, me.id).await? {
        let Some(heir) = List::members_by_standing(&ctx.db, list).await?.first().copied() else {
            // Nobody else is on it, so there is nobody to hand it to and nothing to
            // lose by letting it go with the account.
            continue;
        };

        List::hand_over(&ctx.db, list, heir).await?;
        tracing::info!(list = list.0, to = heir.0, "a shared list changed hands");
    }

    Admitted::forget_user(&ctx.db, me.id).await?;
    User::delete(&ctx.db, me.id).await?;

    tracing::info!(user = me.id.0, "account closed");
    Ok(())
}

/// Every user. The system only — this is for maintenance, not for people.
pub async fn list(
    ctx: &Ctx,
    actor: &Actor,
    page: Paging,
    order_by: OrderBy<user::Field>,
) -> Result<OffsetPage<User>> {
    if !actor.is_system() {
        let person = actor.person()?;
        return Err(ServiceError::hidden("user list", person));
    }
    Ok(User::list(&ctx.db, page, order_by).await?)
}
