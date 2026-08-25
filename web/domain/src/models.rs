#[macro_use]
pub(in crate::models) mod macros;

mod common;
pub mod error;
pub mod history;
pub mod item;
pub mod list;
pub mod note;
pub mod tag;
pub mod unit;
pub mod user;

pub use common::{Direction, OffsetPage, OrderBy, Paging};
pub use error::Error;
pub(in crate::models) use error::Result;

#[cfg(any(test, feature = "test-support"))]
pub use common::tests::pool;

/// The seed data, for transports that want to drive their routers against something
/// realistic.
///
/// Exposed as constants rather than through `seeds!`, which is a `macro_rules!`
/// internal to this crate. Order matters — see `models/fixtures/README.md`.
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures {
    pub const USERS: &str = include_str!("models/fixtures/users.sql");
    pub const LISTS: &str = include_str!("models/fixtures/lists.sql");
    pub const UNITS: &str = include_str!("models/fixtures/units.sql");
    pub const ITEMS: &str = include_str!("models/fixtures/items.sql");
    pub const TAGS: &str = include_str!("models/fixtures/tags.sql");
    pub const NOTES: &str = include_str!("models/fixtures/notes.sql");
}
