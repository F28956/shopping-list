//! Reading what somebody typed.
//!
//! Split out of `domain` so that it can be compiled for a phone. Everything else in
//! `domain` reaches a database, which means sqlx, which means tokio, which means a
//! multi-megabyte static library to answer the question "does `2 kg apples` name a
//! unit". These two modules answer it with the standard library and nothing else, and
//! keeping them in a crate with no dependencies is what keeps that true — a build
//! failure is a better guard than a comment asking people not to.
//!
//! `domain` re-exports both, so `crate::quick_add` still resolves inside it and no
//! caller had to change.

pub mod fuzzy;
pub mod history_rank;
pub mod quick_add;
