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

/// Closes the actor's own account, taking their lists, items and notes with it —
/// every one of those foreign keys is `ON DELETE CASCADE`.
pub async fn close_account(ctx: &Ctx, actor: &Actor) -> Result<()> {
    let me = actor.person()?;
    Ok(User::delete(&ctx.db, me.id).await?)
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
