//! What gets bought on a list, remembered by the list.
//!
//! Keyed on the list rather than the person, so a household sharing a list shares one
//! memory of it: what the shop calls it, what it comes in, and where it belongs. The
//! cost is that a new list starts blank and nothing carries between lists — worth it
//! only because this application pushes towards lists that live a long time, which is
//! what "clear done" is for.
//!
//! Keyed on a normalised name too, so `milk`, `Milk` and `MILK` are one memory, with
//! the spelling last used kept alongside for showing back.

use time::OffsetDateTime;

use super::{Error, Result};
use super::{item, list, tag, unit};

// Scaffold Display, Uses and LastUsedAt
string!(Display);
i64!(Uses);
timestamp!(LastUsedAt);

/// The most entries one list's history will hold.
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
        list_id: list::Id,
        name: &item::Name,
        unit_id: Option<unit::Id>,
    ) -> Result<()> {
        let key = key(name);
        let display = name.0.trim();

        sqlx::query!(
            r#"
            INSERT INTO item_history (list_id, name, display, unit_id)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(list_id, name) DO UPDATE SET
                display      = ?3,
                unit_id      = coalesce(?4, item_history.unit_id),
                uses         = item_history.uses + 1,
                last_used_at = unixepoch()
            "#,
            list_id,
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
    /// Remembers everything an item is filed under, replacing what was there.
    ///
    /// The whole set, not one tag at a time: this is called after an item's tags have
    /// changed, and what it stores is what the item now carries. A column held one,
    /// so attaching a second overwrote the first and the memory looked like it was
    /// forgetting at random.
    ///
    /// Best-effort by its caller: an item that has never been through quick-add has
    /// no entry to hang these on, and that is not a failure to tag it.
    pub async fn remember_tags(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        name: &item::Name,
        tag_ids: &[tag::Id],
    ) -> Result<()> {
        let key = key(name);

        let mut tx = pool.begin().await?;

        // Replaced rather than merged: an item that has just had a tag taken off
        // should not be remembered as still carrying it.
        sqlx::query!(
            r#"DELETE FROM item_history_tags WHERE list_id = ?1 AND name = ?2"#,
            list_id,
            key,
        )
        .execute(&mut *tx)
        .await?;

        for tag_id in tag_ids {
            sqlx::query!(
                r#"
                INSERT INTO item_history_tags (list_id, name, tag_id)
                VALUES (?1, ?2, ?3)
                ON CONFLICT DO NOTHING
                "#,
                list_id,
                key,
                tag_id,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// What this item was filed under last time, in the order a shop is walked.
    pub async fn tags_for(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        name: &item::Name,
    ) -> Result<Vec<tag::Id>> {
        let key = key(name);

        let ids = sqlx::query_scalar!(
            r#"
            SELECT h.tag_id as "tag_id!: tag::Id"
            FROM item_history_tags h
            JOIN tags t ON t.id = h.tag_id
            WHERE h.list_id = ?1 AND h.name = ?2
            ORDER BY t.sort_order, t.name
            "#,
            list_id,
            key,
        )
        .fetch_all(pool)
        .await?;

        Ok(ids)
    }

    /// One entry, if this person has bought it before.
    pub async fn get(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
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
                uses         as "uses: Uses",
                last_used_at as "last_used_at: LastUsedAt"
            FROM item_history
            WHERE list_id = ?1 AND name = ?2
            "#,
            list_id,
            key
        )
        .fetch_optional(pool)
        .await?)
    }

    /// This list's whole history, newest use first.
    ///
    /// The order here is only a bound on how much is read — see
    /// [`crate::history_rank`] for the order it is offered in. Recency is the right
    /// thing to bound by: an entry not touched in years is the one that matters least.
    pub async fn for_list(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        limit: i64,
    ) -> Result<Vec<Entry>> {
        Ok(sqlx::query_as!(
            Entry,
            r#"
            SELECT
                name         as "name: item::Name",
                display      as "display: Display",
                unit_id      as "unit_id?: unit::Id",
                uses         as "uses: Uses",
                last_used_at as "last_used_at: LastUsedAt"
            FROM item_history
            WHERE list_id = ?1
            ORDER BY last_used_at DESC
            LIMIT ?2
            "#,
            list_id,
            limit
        )
        .fetch_all(pool)
        .await?)
    }

    /// Drops one remembered item — the way back from a typo.
    pub async fn forget(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        name: &item::Name,
    ) -> Result<()> {
        let key = key(name);

        let result = sqlx::query!(
            r#"DELETE FROM item_history WHERE list_id = ?1 AND name = ?2"#,
            list_id,
            key
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Trims a list's history back to [`MAX_ENTRIES`], dropping the least-used and
    /// least-recent first, and says how many went.
    ///
    /// Called after recording rather than on a schedule: the only moment the table
    /// can grow is the moment something was added to it.
    pub async fn prune(pool: &sqlx::SqlitePool, list_id: list::Id) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM item_history
            WHERE list_id = ?1 AND name IN (
                SELECT name FROM item_history
                WHERE list_id = ?1
                ORDER BY uses DESC, last_used_at DESC
                LIMIT -1 OFFSET ?2
            )
            "#,
            list_id,
            MAX_ENTRIES
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}
