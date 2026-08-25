//! What a person buys, remembered across the lists they bought it on.
//!
//! Keyed on a normalised name, so `milk`, `Milk` and `MILK` are one memory, with the
//! spelling they last used kept alongside for showing back.

use time::OffsetDateTime;

use super::{Error, Result};
use super::{item, tag, unit, user};

// Scaffold Display, Uses and LastUsedAt
string!(Display);
i64!(Uses);
timestamp!(LastUsedAt);

/// The most entries one person's history will hold.
///
/// Uncapped it would grow by every typo forever. Five hundred is far more than a
/// household buys and small enough that the whole table stays cheap to read.
pub const MAX_ENTRIES: i64 = 500;

/// One remembered item.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Entry {
    /// The normalised key — trimmed and lowercased.
    pub name: item::Name,
    /// The spelling last used, for showing back.
    pub display: Display,
    pub unit_id: Option<unit::Id>,
    pub tag_id: Option<tag::Id>,
    pub uses: Uses,
    pub last_used_at: LastUsedAt,
}

/// The key an entry is stored under: trimmed, then lowercased.
///
/// In Rust rather than SQL because SQLite's `lower()` folds ASCII only — the same
/// reason [`super::unit`] normalises here.
pub fn key(name: &item::Name) -> String {
    name.0.trim().to_lowercase()
}

impl Entry {
    /// Records a use of `name`, creating the entry or bumping it.
    ///
    /// `unit_id` overwrites what was remembered when it is `Some`, and leaves it
    /// alone when it is `None`: adding `milk` with no unit should not forget that
    /// milk comes in pints, but adding `2 litre milk` should update it.
    ///
    /// One statement, so two lists being added to at once cannot race into two rows
    /// or a lost count.
    pub async fn record(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
        name: &item::Name,
        unit_id: Option<unit::Id>,
    ) -> Result<()> {
        let key = key(name);
        let display = name.0.trim();

        sqlx::query!(
            r#"
            INSERT INTO item_history (user_id, name, display, unit_id)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(user_id, name) DO UPDATE SET
                display      = ?3,
                unit_id      = coalesce(?4, item_history.unit_id),
                uses         = item_history.uses + 1,
                last_used_at = unixepoch()
            "#,
            user_id,
            key,
            display,
            unit_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Remembers which category this item belongs in.
    ///
    /// Separate from [`Entry::record`] because tagging happens after the item exists,
    /// and because a tag being removed should not count as another use.
    pub async fn remember_tag(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
        name: &item::Name,
        tag_id: Option<tag::Id>,
    ) -> Result<()> {
        let key = key(name);

        sqlx::query!(
            r#"UPDATE item_history SET tag_id = ?3 WHERE user_id = ?1 AND name = ?2"#,
            user_id,
            key,
            tag_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// One entry, if this person has bought it before.
    pub async fn get(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
        name: &item::Name,
    ) -> Result<Option<Entry>> {
        let key = key(name);

        Ok(sqlx::query_as!(
            Entry,
            r#"
            SELECT
                name         as "name: item::Name",
                display      as "display: Display",
                unit_id      as "unit_id?: unit::Id",
                tag_id       as "tag_id?: tag::Id",
                uses         as "uses: Uses",
                last_used_at as "last_used_at: LastUsedAt"
            FROM item_history
            WHERE user_id = ?1 AND name = ?2
            "#,
            user_id,
            key
        )
        .fetch_optional(pool)
        .await?)
    }

    /// This person's whole history, newest use first.
    ///
    /// The order here is only a bound on how much is read — see
    /// [`crate::history_rank`] for the order it is offered in. Recency is the right
    /// thing to bound by: an entry not touched in years is the one that matters least.
    pub async fn for_user(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
        limit: i64,
    ) -> Result<Vec<Entry>> {
        Ok(sqlx::query_as!(
            Entry,
            r#"
            SELECT
                name         as "name: item::Name",
                display      as "display: Display",
                unit_id      as "unit_id?: unit::Id",
                tag_id       as "tag_id?: tag::Id",
                uses         as "uses: Uses",
                last_used_at as "last_used_at: LastUsedAt"
            FROM item_history
            WHERE user_id = ?1
            ORDER BY last_used_at DESC
            LIMIT ?2
            "#,
            user_id,
            limit
        )
        .fetch_all(pool)
        .await?)
    }

    /// Drops one remembered item — the way back from a typo.
    pub async fn forget(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
        name: &item::Name,
    ) -> Result<()> {
        let key = key(name);

        let result = sqlx::query!(
            r#"DELETE FROM item_history WHERE user_id = ?1 AND name = ?2"#,
            user_id,
            key
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Trims the history back to [`MAX_ENTRIES`], dropping the least-used and
    /// least-recent first, and says how many went.
    ///
    /// Called after recording rather than on a schedule: the only moment the table
    /// can grow is the moment something was added to it.
    pub async fn prune(pool: &sqlx::SqlitePool, user_id: user::Id) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM item_history
            WHERE user_id = ?1 AND name IN (
                SELECT name FROM item_history
                WHERE user_id = ?1
                ORDER BY uses DESC, last_used_at DESC
                LIMIT -1 OFFSET ?2
            )
            "#,
            user_id,
            MAX_ENTRIES
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}
