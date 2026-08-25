//! Units are shared reference data: everyone reads them, nobody owns them, and only
//! the process itself writes them.
//!
//! That asymmetry is the point. `kg` is not anyone's kilogram, and letting one person
//! rename it would rename it on every other person's list — so editing is
//! [`Actor::System`]'s, which no request can produce.

use crate::models::unit::{self, Name, Unit};
use crate::models::{OffsetPage, OrderBy, Paging};

use super::{Actor, Ctx, Result, ServiceError};

/// Anyone signed in may read the unit list.
/// Readable by any actor: there is no owner to check against, and a unit or a tag
/// tells you nothing about anybody. The `actor` argument stays for the signature all
/// service calls share, and because a future rule would land here.
pub async fn list(
    ctx: &Ctx,
    _actor: &Actor,
    page: Paging,
    order_by: OrderBy<unit::Field>,
) -> Result<OffsetPage<Unit>> {
    Ok(Unit::list(&ctx.db, page, order_by).await?)
}

pub async fn get(ctx: &Ctx, _actor: &Actor, by: unit::Lookup) -> Result<Unit> {
    Ok(Unit::get(&ctx.db, by).await?)
}

pub async fn create(ctx: &Ctx, actor: &Actor, name: Name) -> Result<Unit> {
    writable(actor)?;
    Ok(Unit::create(&ctx.db, name).await?)
}

pub async fn update(ctx: &Ctx, actor: &Actor, id: unit::Id, name: Name) -> Result<Unit> {
    writable(actor)?;
    Ok(Unit::update(&ctx.db, id, name).await?)
}

/// Deleting a unit an item still uses is [`ServiceError::InUse`] — `items.unit_id` is
/// `ON DELETE RESTRICT`, because a unit outlives any one item.
pub async fn delete(ctx: &Ctx, actor: &Actor, id: unit::Id) -> Result<()> {
    writable(actor)?;
    Ok(Unit::delete(&ctx.db, id).await?)
}

/// The system only. A person gets `NotFound` rather than a distinct refusal, for the
/// same reason as everywhere else: the answer must not depend on what exists.
fn writable(actor: &Actor) -> Result<()> {
    if actor.is_system() {
        return Ok(());
    }
    if let Ok(person) = actor.person() {
        return Err(ServiceError::forbidden("unit (write)", person));
    }
    Err(ServiceError::Unauthenticated)
}
