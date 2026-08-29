//! What the process says about itself: a log somebody reads, and numbers something
//! scrapes.
//!
//! Two rules shape everything here, and both come out of `docs/self-hosting.md` S8.
//! The person with root is the user, so there is no operator to hide things from —
//! but a shopping list is more revealing than it looks, and a log file is the copy of
//! it that ends up pasted into an issue, shipped to a hosted log service, or left on
//! a disk somebody sells.
//!
//! **Rule one: `info` and above never carry contents.** Not by convention — by
//! construction. Anything that names an item, a list, an address, a token or an
//! invite code goes through [`contents!`], which can only produce a `trace` event on
//! the [`CONTENTS`] target, and that target is switched off unless the operator asked
//! for `debug` or `trace`. There is no spelling of `contents!` that emits at `info`,
//! because the macro writes the level itself.
//!
//! **Rule two: a label is never a row.** Every metric recorded here is recorded by one
//! of the functions in [`instruments`], and none of them accepts a list id, an item
//! uuid or an address. Route labels are the *pattern* — `/api/lists/{list_id}/items`
//! — never the path that was asked for. A metric labelled by list id would put a
//! per-household series in a scrape endpoint, which is both a cardinality problem and
//! a disclosure one.

pub mod http;
pub mod instruments;

pub use http::record_requests;
pub use instruments::{SseStream, instruments};

/// The one tracing target on which list and item contents may appear.
///
/// Separate from the module path deliberately. A target is the only field of an event
/// a subscriber can filter on without reading its values, so making it the carrier is
/// what lets `server::logging` drop every one of these with a single predicate,
/// whatever `RUST_LOG` says.
pub const CONTENTS: &str = "contents";

/// Logs something a person typed, at `trace`, on the [`CONTENTS`] target.
///
/// The level is written by the macro and not by the caller. That is the whole point:
/// `contents!(name = %item.name, "added")` is the only way to say this, and it cannot
/// be turned into an `info` by editing it — the argument list has nowhere to put a
/// level. A reviewer looking for a leak greps for four things (`info!`, `warn!`,
/// `error!`, and this) rather than reading every field of every log site.
///
/// Use it for anything derived from what somebody entered: item and list names, the
/// raw line a quick-add parsed, addresses, tokens, invite codes, sync payloads.
#[macro_export]
macro_rules! contents {
    ($($arg:tt)*) => {
        ::tracing::trace!(target: $crate::CONTENTS, $($arg)*)
    };
}
