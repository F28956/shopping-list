//! The domain: what a shopping list is, and who may do what to it.
//!
//! Everything here is transport-agnostic. The HTTP API, the server-rendered web UI
//! and (later) MCP are adapters over this crate, and none of them may reach past it
//! to the database.

pub mod history_rank;
pub mod models;
// Re-exported rather than defined here: both are pure, and living in a crate with no
// dependencies is what lets the phones compile them. See `parsing`.
pub use parsing::{fuzzy, quick_add};
pub mod reference;
pub mod service;

/// The schema, embedded at compile time.
///
/// Migrations live beside the models they describe rather than beside any one
/// transport, so which process runs them stays a deployment decision.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();
