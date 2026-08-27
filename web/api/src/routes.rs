//! HTTP shapes. Everything below is translation: request into service arguments,
//! service result into JSON. No route makes an access decision of its own — that
//! belongs to `domain::service`, so the browser and MCP get the same answers.

#[cfg(test)]
mod tests;

pub mod history;
pub mod items;
pub mod lists;
pub mod me;
pub mod notes;
pub mod reference;
pub mod sessions;
pub mod sharing;
pub mod sync;

use domain::models::{Direction, OrderBy, Paging};

/// Paging and ordering, as they arrive on the query string.
///
/// `?page=2&size=20&order_by=created_at&direction=descending`. Every field is
/// optional; the defaults are the first page of twenty, newest ordering left to the
/// caller's field of choice.
#[derive(Debug, serde::Deserialize)]
pub struct PageQuery<F> {
    #[serde(default = "first_page")]
    pub page: i64,
    #[serde(default = "default_size")]
    pub size: i64,
    /// Defaulted rather than required. A list route that 422s until the caller
    /// guesses a field is a route every new client meets as a bug first, and each
    /// field's default is the order that transport reads it in anyway.
    #[serde(default)]
    pub order_by: F,
    #[serde(default = "ascending")]
    pub direction: Direction,
}

fn first_page() -> i64 {
    1
}

/// Bounded, because `size` reaches SQLite as a LIMIT and a caller should not be able
/// to ask for the whole table in one request.
fn default_size() -> i64 {
    20
}

/// The same ceiling the service layer uses, so a client cannot discover a different
/// one by asking. The default page is smaller; this is only the limit.
const MAX_SIZE: i64 = domain::service::PAGE_MAX;

fn ascending() -> Direction {
    Direction::Ascending
}

impl<F: Copy> PageQuery<F> {
    pub fn paging(&self) -> Paging {
        Paging {
            number: self.page,
            size: self.size.clamp(0, MAX_SIZE),
        }
    }

    pub fn order_by(&self) -> OrderBy<F> {
        OrderBy {
            field: self.order_by,
            direction: self.direction,
        }
    }
}
