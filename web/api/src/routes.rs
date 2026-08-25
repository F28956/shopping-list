//! HTTP shapes. Everything below is translation: request into service arguments,
//! service result into JSON. No route makes an access decision of its own — that
//! belongs to `domain::service`, so the browser and MCP get the same answers.

#[cfg(test)]
mod tests;

pub mod items;
pub mod lists;
pub mod me;
pub mod notes;
pub mod reference;

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

const MAX_SIZE: i64 = 100;

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
