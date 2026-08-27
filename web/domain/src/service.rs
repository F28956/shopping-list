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
#[cfg(test)]
mod sync_tests;

pub mod admission;
pub mod changes;
pub mod identity;
pub mod items;
pub mod lists;
pub mod notes;
pub mod sessions;
pub mod sync;
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
    /// Who to tell when a list changes. Clone a `Ctx` to share it; construct two and
    /// the transports are watching separate worlds.
    pub changes: changes::Changes,
    /// What has to be presented to claim an unclaimed server, printed to the log at
    /// boot and held only in memory.
    ///
    /// `None` means no claim is possible, which is the safe default and the state of
    /// every server that has already been claimed. It is here rather than in a
    /// transport because the rule it belongs to — only an unclaimed server, and only
    /// with this code — is one rule, and D1 says rules live in one place.
    ///
    /// Not stored: a new code on every restart is a feature. It expires by the process
    /// ending, there is no hash to keep, and the log a self-hoster is already looking
    /// at has the current one.
    pub claim_code: Option<String>,
}

impl Ctx {
    /// A context over this pool.
    ///
    /// Says nothing about who may sign in: that is rows now, not a field, so that
    /// changing it is something a person does through the app rather than a redeploy.
    /// See [`admission`].
    pub fn new(db: sqlx::SqlitePool) -> Self {
        Self {
            db,
            changes: changes::Changes::new(),
            claim_code: None,
        }
    }

    /// The same context, willing to be claimed by whoever presents this code.
    pub fn awaiting_claim(self, code: String) -> Self {
        Self {
            claim_code: Some(code),
            ..self
        }
    }
}

pub type Result<T, E = ServiceError> = std::result::Result<T, E>;

/// The most rows any one request will return.
///
/// One definition, because four of them drifting apart is how a caller ends up
/// truncating at a number nobody chose. A transport may ask for less; it cannot ask
/// for more, since `Paging` reaches SQLite as a LIMIT.
pub const PAGE_MAX: i64 = 500;

/// A single page of everything, up to [`PAGE_MAX`].
pub fn everything() -> crate::models::Paging {
    crate::models::Paging {
        number: 1,
        size: PAGE_MAX,
    }
}

/// Alphabetical, for a list a person reads.
/// Tags in the order a shop is walked, which is what grouping a list means.
pub(crate) fn by_shop() -> crate::models::OrderBy<crate::models::tag::Field> {
    crate::models::OrderBy {
        field: crate::models::tag::Field::SortOrder,
        direction: crate::models::Direction::Ascending,
    }
}

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
    /// The actor cannot act as a person at all.
    #[error("unauthenticated")]
    Unauthenticated,
    /// A person who may see the thing but not do this to it — a viewer trying to
    /// edit. Distinct from `NotFound`, which is what someone who may not see it at
    /// all gets; see [`ServiceError::forbidden`].
    #[error("forbidden")]
    Forbidden,
    /// Not a user of this service at all: the identity provider vouched for them and
    /// the admission list does not list them.
    ///
    /// Separate from [`ServiceError::Forbidden`], which it used to share, because the
    /// two are answers to different questions and a transport has to say different
    /// things about them. `Forbidden` is "you may look at this list but not change
    /// it", which is a sentence about a list; this is "this account cannot use this
    /// server", which is a sentence about the account and is not fixed by asking
    /// again. Collapsing them is how a stranger signing in was told, on a screen with
    /// no list on it, that they could look at the list but not change it.
    #[error("not admitted")]
    NotAdmitted,
    #[error(transparent)]
    Internal(models::Error),
}

impl ServiceError {
    /// The answer to "you cannot see this".
    ///
    /// [`ServiceError::NotFound`], never `Forbidden`: confirming the row exists tells
    /// someone holding a guessed id something true about another person's data. The
    /// distinction is kept in the log line, not in the response.
    fn hidden(what: &str, actor: &user::User) -> Self {
        tracing::warn!(user = actor.id.0, resource = what, "access refused");
        ServiceError::NotFound
    }

    /// The answer to "you may see this, but you may not do that to it".
    ///
    /// A viewer on a shared list already knows it exists, so pretending otherwise is
    /// a lie that reads as a bug. This is the only case where the distinction is safe
    /// to make, and it exists only because roles do.
    fn refused(what: &str, actor: &user::User) -> Self {
        tracing::info!(user = actor.id.0, resource = what, "insufficient role");
        ServiceError::Forbidden
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
            | (Self::Unauthenticated, Self::Unauthenticated)
            | (Self::Forbidden, Self::Forbidden)
            | (Self::NotAdmitted, Self::NotAdmitted) => true,
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
