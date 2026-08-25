//! What a caller may do, as opposed to what the database can store.
//!
//! Models are transport-agnostic and actor-agnostic: [`models::note::Note::get`] will
//! happily hand you anybody's note, because a row does not know who is asking. This
//! layer is where that question is answered, and it is the only layer that answers
//! it. Transports authenticate — they turn a bearer token or a session cookie into an
//! [`Actor`] — and then call in here. A transport that reaches past this module to a
//! model has skipped every access check with it.

#[cfg(test)]
mod authorization_tests;

pub mod items;
pub mod lists;
pub mod notes;
pub mod tags;
pub mod units;
pub mod users;

use crate::models::{self, user};

/// Who is asking.
///
/// Produced by transports from a verified identity, and consumed by every service
/// function. There is deliberately no `Anonymous` variant: a request without a
/// verified identity never gets an `Actor` at all, so "not signed in" is a shape the
/// service layer cannot be handed.
#[derive(Debug, Clone)]
pub enum Actor {
    /// A signed-in person, acting for themselves.
    User(user::User),
    /// The process itself — fixtures, migrations, maintenance. Never constructed from
    /// a request, and never able to act as a particular person: operations scoped to
    /// "my things" reject it, because it has no things.
    System,
}

impl Actor {
    /// The person acting, or [`ServiceError::Unauthenticated`] if this is the system.
    ///
    /// Used by every operation scoped to an owner. The system has no lists and no
    /// notes, so there is nothing sensible for it to mean here.
    pub fn person(&self) -> Result<&user::User> {
        match self {
            Actor::User(user) => Ok(user),
            Actor::System => Err(ServiceError::Unauthenticated),
        }
    }

    /// Whether this actor may write shared reference data — units and tags, which
    /// belong to no one and are edited out of band rather than by users.
    pub fn is_system(&self) -> bool {
        matches!(self, Actor::System)
    }
}

/// What a service call needs from the process.
///
/// A struct rather than a bare pool so that adding a clock, a job queue or a metrics
/// handle later does not mean changing every signature in this module.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub db: sqlx::SqlitePool,
}

impl Ctx {
    pub fn new(db: sqlx::SqlitePool) -> Self {
        Self { db }
    }
}

pub type Result<T, E = ServiceError> = std::result::Result<T, E>;

/// Everything, for the reference tables that are small by construction.
pub(crate) fn everything() -> crate::models::Paging {
    crate::models::Paging {
        number: 1,
        size: 500,
    }
}

/// Alphabetical, for a list a person reads.
pub(crate) fn by_name<F: NamedField>() -> crate::models::OrderBy<F> {
    crate::models::OrderBy {
        field: F::NAME,
        direction: crate::models::Direction::Ascending,
    }
}

/// The `Name` variant of a model's sortable fields.
pub(crate) trait NamedField: Copy {
    const NAME: Self;
}

impl NamedField for crate::models::unit::Field {
    const NAME: Self = crate::models::unit::Field::Name;
}

impl NamedField for crate::models::tag::Field {
    const NAME: Self = crate::models::tag::Field::Name;
}

/// What can go wrong, in the caller's terms rather than the database's.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// No such thing — or none this actor may see. The two are deliberately the same
    /// answer; see the note on [`ServiceError::forbidden`].
    #[error("not found")]
    NotFound,
    #[error("already exists")]
    Conflict,
    #[error("in use")]
    InUse,
    #[error("invalid input")]
    InvalidInput,
    /// The actor cannot act as a person at all. Distinct from an actor who is a
    /// person but may not touch this particular row — that is `NotFound`.
    #[error("unauthenticated")]
    Unauthenticated,
    #[error(transparent)]
    Internal(models::Error),
}

impl ServiceError {
    /// The answer to "you may not touch this".
    ///
    /// It is [`ServiceError::NotFound`], never a distinct `Forbidden`. `Forbidden`
    /// confirms the row exists, which tells someone holding a guessed id something
    /// true about another person's data. The distinction is kept in the log line, not
    /// in the response.
    fn forbidden(what: &str, actor: &user::User) -> Self {
        tracing::warn!(user = actor.id.0, resource = what, "access refused");
        ServiceError::NotFound
    }
}

impl From<models::Error> for ServiceError {
    fn from(err: models::Error) -> Self {
        match err {
            models::Error::NotFound => Self::NotFound,
            models::Error::Conflict => Self::Conflict,
            models::Error::InUse => Self::InUse,
            models::Error::InvalidInput => Self::InvalidInput,
            other => Self::Internal(other),
        }
    }
}

impl PartialEq for ServiceError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::NotFound, Self::NotFound)
            | (Self::Conflict, Self::Conflict)
            | (Self::InUse, Self::InUse)
            | (Self::InvalidInput, Self::InvalidInput)
            | (Self::Unauthenticated, Self::Unauthenticated) => true,
            (Self::Internal(a), Self::Internal(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
pub(in crate::service) mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::models::user::{Name, Sub, User};

    /// A signed-in person, created the way authentication would create them.
    pub(in crate::service) async fn person(pool: &SqlitePool, sub: &str) -> Actor {
        let user = User::find_or_create(pool, Sub(sub.into()), Some(Name(sub.into())), None)
            .await
            .expect("could not create the test user");
        Actor::User(user)
    }

    #[test]
    fn system_cannot_act_as_a_person() {
        assert_eq!(
            Actor::System.person().unwrap_err(),
            ServiceError::Unauthenticated
        );
        assert!(Actor::System.is_system());
    }
}
