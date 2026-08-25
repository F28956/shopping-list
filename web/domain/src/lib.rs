//! The domain: what a shopping list is, and who may do what to it.
//!
//! Everything here is transport-agnostic. The HTTP API, the server-rendered web UI
//! and (later) MCP are adapters over this crate, and none of them may reach past it
//! to the database.

pub mod models;

/// The schema, embedded at compile time.
///
/// Migrations live beside the models they describe rather than beside any one
/// transport, so which process runs them stays a deployment decision.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();
