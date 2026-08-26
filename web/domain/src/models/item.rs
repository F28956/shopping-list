use time::OffsetDateTime;

use super::{Error, Result};
use super::{OffsetPage, OrderBy, Paging};
use super::{list, unit, user};

// Scaffold Id, Name, Amount, DoneAt and CreatedAt
i64!(Id);
string!(Name);
f64!(Amount);
timestamp!(DoneAt);
timestamp!(CreatedAt);

// An item name is free text a person reads back, so only the padding comes off
trimmed!(Name);
capitalised!(Name);

/// The longest name `items.name` accepts, in characters — keep in step with the
/// `CHECK` in the init migration. Anything longer is [`Error::InvalidInput`].
pub const MAX_NAME: usize = 128;

/// A line on a shopping list.
///
/// `unit_id` is optional — "a birthday cake" needs no unit — and `done_at` doubles as
/// the done flag: `None` is outstanding, `Some` is when it was ticked off. There is
/// no separate boolean to fall out of step with the timestamp.
///
/// The `unit_id?:`/`done_at?:` annotations on the queries below are load-bearing.
/// Where a `CHECK` or a `RETURNING` makes sqlx infer a nullable column as NOT NULL, a
/// `#[sqlx(transparent)]` newtype decodes the NULL as `Some(T(default))` rather than
/// `None` — silently. The `?` forces the nullable decode back on.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Item {
    pub id: Id,
    pub list_id: list::Id,
    pub name: Name,
    pub amount: Amount,
    pub unit_id: Option<unit::Id>,
    pub done_at: Option<DoneAt>,
    pub created_at: CreatedAt,
}

/// How a caller asks for a single item. Only `id` identifies one: item names repeat
/// across lists, and nothing about an item is unique.
#[derive(Debug, Clone)]
pub enum Lookup {
    Id(Id),
}

/// What `for_list` may order by. Deliberately a separate enum from [`Lookup`] — the
/// set of sortable columns and the set of unique keys are not the same set.
///
/// `list_id` is absent on purpose: `for_list` scopes to one list, so within a page it
/// is a constant and ordering by it would do nothing.
///
/// Every variant added here needs a matching `WHEN` arm in both `CASE` branches of
/// the query. A variant without one silently sorts by nothing, which is what
/// `for_list_every_field_changes_the_order` exists to catch.
/// The default is `DoneAt`: outstanding first, then what is already in the trolley -- the order a
    /// list is read in.
///
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Id,
    Name,
    Amount,
    #[default]
    DoneAt,
    CreatedAt,
}

impl Item {
    /// Adds an item to a list, outstanding.
    ///
    /// A `list_id` or `unit_id` that matches nothing is [`Error::InvalidInput`], as
    /// is an `amount` that is not greater than zero.
    /// The item on this list that a new one would be another of, if there is one.
    ///
    /// Same name, ignoring case and surrounding space, and the same unit. The unit
    /// has to match because the amounts are about to be added together: three of
    /// something and two kilograms of it are not five of anything.
    ///
    /// Matched in Rust rather than in SQL. SQLite's `lower()` and `COLLATE NOCASE`
    /// are ASCII-only, so `Ångström` and `ångström` would come back as two different
    /// things — the same trap that moved unit normalisation out of the database.
    ///
    /// An outstanding row wins over a crossed-off one: adding milk when milk is on
    /// the list means the one you still need, not the one already in the trolley.
    pub async fn alike(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        name: &Name,
        unit_id: Option<unit::Id>,
    ) -> Result<Option<Item>> {
        let wanted = name.0.trim().to_lowercase();

        let candidates = sqlx::query_as!(
            Item,
            r#"
            SELECT
                id          as "id!: Id",
                list_id     as "list_id: list::Id",
                name        as "name: Name",
                amount      as "amount: Amount",
                unit_id     as "unit_id?: unit::Id",
                done_at     as "done_at?: DoneAt",
                created_at  as "created_at!: CreatedAt"
            FROM items
            WHERE list_id = ?1
            ORDER BY done_at IS NOT NULL, created_at
            "#,
            list_id,
        )
        .fetch_all(pool)
        .await?;

        Ok(candidates
            .into_iter()
            .find(|i| i.unit_id == unit_id && i.name.0.trim().to_lowercase() == wanted))
    }

    /// Adds to an item's amount, and puts it back on the list if it was crossed off.
    ///
    /// Un-crossing is the point as much as the arithmetic: adding something you have
    /// already ticked off is how you say you need it after all.
    pub async fn add_to(pool: &sqlx::SqlitePool, id: Id, extra: Amount) -> Result<Item> {
        let item = sqlx::query_as!(
            Item,
            r#"
            UPDATE items
            SET amount = amount + ?2, done_at = NULL
            WHERE id = ?1
            RETURNING
                id          as "id!: Id",
                list_id     as "list_id: list::Id",
                name        as "name: Name",
                amount      as "amount: Amount",
                unit_id     as "unit_id?: unit::Id",
                done_at     as "done_at?: DoneAt",
                created_at  as "created_at!: CreatedAt"
            "#,
            id,
            extra,
        )
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;

        Ok(item)
    }

    pub async fn create(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        name: Name,
        amount: Amount,
        unit_id: Option<unit::Id>,
    ) -> Result<Item> {
        let name = name.trimmed().capitalised();

        let item = sqlx::query_as!(
            Item,
            r#"
            INSERT INTO items (list_id, name, amount, unit_id)
            VALUES (?1, ?2, ?3, ?4)
            RETURNING
                id          as "id!: Id",
                list_id     as "list_id: list::Id",
                name        as "name: Name",
                amount      as "amount: Amount",
                unit_id     as "unit_id?: unit::Id",
                done_at     as "done_at?: DoneAt",
                created_at  as "created_at!: CreatedAt"
            "#,
            list_id,
            name,
            amount,
            unit_id,
        )
        .fetch_one(pool)
        .await?;

        Ok(item)
    }

    /// Replaces what a person typed: name, amount, unit.
    ///
    /// `done_at` is not here and `list_id` is not either — ticking an item off is
    /// [`Item::set_done`], and moving one between lists is a different operation that
    /// would need the destination checked.
    pub async fn update(
        pool: &sqlx::SqlitePool,
        id: Id,
        name: Name,
        amount: Amount,
        unit_id: Option<unit::Id>,
    ) -> Result<Item> {
        let name = name.trimmed().capitalised();

        let item = sqlx::query_as!(
            Item,
            r#"
            UPDATE items SET name = ?1, amount = ?2, unit_id = ?3 WHERE id = ?4
            RETURNING
                id          as "id!: Id",
                list_id     as "list_id: list::Id",
                name        as "name: Name",
                amount      as "amount: Amount",
                unit_id     as "unit_id?: unit::Id",
                done_at     as "done_at?: DoneAt",
                created_at  as "created_at: CreatedAt"
            "#,
            name,
            amount,
            unit_id,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(item)
    }

    /// Ticks an item off, or puts it back.
    ///
    /// Stamping is idempotent in effect but not in value: ticking an item that is
    /// already done restamps it with the later time, which is the honest answer to
    /// "when was this done" for the tick that actually happened.
    pub async fn set_done(pool: &sqlx::SqlitePool, id: Id, done: bool) -> Result<Item> {
        let item = sqlx::query_as!(
            Item,
            r#"
            UPDATE items SET done_at = CASE WHEN ?1 THEN unixepoch() ELSE NULL END
            WHERE id = ?2
            RETURNING
                id          as "id!: Id",
                list_id     as "list_id: list::Id",
                name        as "name: Name",
                amount      as "amount: Amount",
                unit_id     as "unit_id?: unit::Id",
                done_at     as "done_at?: DoneAt",
                created_at  as "created_at: CreatedAt"
            "#,
            done,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(item)
    }

    /// Deletes an item. Nothing references an item except its tags, which are
    /// `ON DELETE CASCADE`, so this is never [`Error::InUse`].
    pub async fn delete(pool: &sqlx::SqlitePool, id: Id) -> Result<()> {
        let result = sqlx::query!(r#"DELETE FROM items WHERE id = ?1"#, id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Fetches one page of a list's items.
    ///
    /// Items are scoped to their list, so there is no unscoped `list`: an endpoint
    /// that could page over every item in the table is one refactor away from leaking
    /// them. The `items_by_list` index covers the filter.
    ///
    /// `total` counts that list's items, not the table's, and is a second statement —
    /// see [`super::unit::Unit::list`] for why it is not folded into the page query.
    pub async fn for_list(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
        page: Paging,
        order_by: OrderBy<Field>,
    ) -> Result<OffsetPage<Item>> {
        let field: &'static str = order_by.field.into();
        let direction: &'static str = order_by.direction.into();

        let limit = page.limit();
        let offset = page.offset();

        let items = sqlx::query_as!(
            Item,
            r#"
        SELECT
            id          as "id: Id",
            list_id     as "list_id: list::Id",
            name        as "name: Name",
            amount      as "amount: Amount",
            unit_id     as "unit_id?: unit::Id",
            done_at     as "done_at?: DoneAt",
            created_at  as "created_at: CreatedAt"
        FROM items
        WHERE list_id = ?1
        ORDER BY
            CASE
                WHEN ?3 = 'ascending' THEN
                    CASE ?2
                        WHEN 'id' THEN id
                        WHEN 'name' THEN name
                        WHEN 'amount' THEN amount
                        WHEN 'done_at' THEN done_at
                        WHEN 'created_at' THEN created_at
                    END
                END ASC NULLS LAST,
            CASE
                WHEN ?3 = 'descending' THEN
                    CASE ?2
                        WHEN 'id' THEN id
                        WHEN 'name' THEN name
                        WHEN 'amount' THEN amount
                        WHEN 'done_at' THEN done_at
                        WHEN 'created_at' THEN created_at
                    END
            END DESC NULLS LAST,
            -- keeps paging deterministic when the sort key ties
            id ASC
        LIMIT ?4 OFFSET ?5
        "#,
            list_id,
            field,
            direction,
            limit,
            offset,
        )
        .fetch_all(pool)
        .await?;

        let total = sqlx::query_scalar!(
            r#"SELECT count(*) as "total!: i64" FROM items WHERE list_id = ?1"#,
            list_id
        )
        .fetch_one(pool)
        .await?;

        Ok(page.page_of(items, total))
    }

    /// The distinct item names this person has used before, most-used first.
    ///
    /// Retyping `Milk` every week is the complaint people actually have about list
    /// apps, and every answer to it is already in this table. Scoped by owner rather
    /// than by list: what you buy is a property of you, not of one list, and the
    /// suggestion is most useful on a list that is still empty.
    ///
    /// Grouped `COLLATE NOCASE` so `milk` and `Milk` are one suggestion; the spelling
    /// returned is whichever the database picked, since either is one the person has
    /// typed themselves.
    pub async fn suggestions(
        pool: &sqlx::SqlitePool,
        owner_id: user::Id,
        limit: i64,
    ) -> Result<Vec<Name>> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT i.name as "name!: Name"
            FROM items i
            JOIN lists l ON l.id = i.list_id
            WHERE l.owner_id = ?1
            GROUP BY i.name COLLATE NOCASE
            ORDER BY count(*) DESC, i.name
            LIMIT ?2
            "#,
            owner_id,
            limit
        )
        .fetch_all(pool)
        .await?)
    }

    /// Removes everything already ticked off a list, and says how many went.
    ///
    /// One statement rather than a delete per item: clearing up after a shop is one
    /// action to the person doing it, and twenty round trips is a visible pause.
    pub async fn delete_done(pool: &sqlx::SqlitePool, list_id: list::Id) -> Result<u64> {
        let result = sqlx::query!(
            r#"DELETE FROM items WHERE list_id = ?1 AND done_at IS NOT NULL"#,
            list_id
        )
        .execute(pool)
        .await?;

        // No rows is not a miss: clearing a list with nothing ticked is a no-op, not
        // an error, and the button that calls this is allowed to be pressed twice.
        Ok(result.rows_affected())
    }

    /// Fetches one item. A miss is [`Error::NotFound`], not `Ok(None)`.
    ///
    /// This does not scope to a list, so a caller holding an id from someone else's
    /// list gets their item — checking that the item is on a list the requester may
    /// see is the caller's job, and `list_id` is on the row so it can.
    pub async fn get(pool: &sqlx::SqlitePool, by: Lookup) -> Result<Item> {
        let item = match by {
            Lookup::Id(v) => {
                sqlx::query_as!(
                    Item,
                    r#"
                SELECT
                    id          as "id: Id",
                    list_id     as "list_id: list::Id",
                    name        as "name: Name",
                    amount      as "amount: Amount",
                    unit_id     as "unit_id?: unit::Id",
                    done_at     as "done_at?: DoneAt",
                    created_at  as "created_at: CreatedAt"
                FROM items
                WHERE id = ?1 "#,
                    v
                )
                .fetch_one(pool)
                .await?
            }
        };

        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;
    use strum::VariantArray;

    use super::*;
    use crate::models::{Direction, pool};

    /// Items in `fixtures/items.sql`.
    const SEEDED: i64 = 73;
    /// The fullest list in the fixture, and how many items are on it.
    const BUSIEST: &str = "Fruit & veg";
    const BUSIEST_ITEMS: i64 = 9;
    /// The one item in the fixture with no unit.
    const UNITLESS: &str = "Cake candles";

    /// Everything the fixture seeds is reached through a list.
    async fn list_id(pool: &SqlitePool, name: &str) -> Result<list::Id> {
        Ok(sqlx::query_scalar!(
            r#"SELECT id as "id!: list::Id" FROM lists WHERE name = ?1"#,
            name
        )
        .fetch_one(pool)
        .await?)
    }

    async fn unit_id(pool: &SqlitePool, name: &str) -> Result<unit::Id> {
        Ok(sqlx::query_scalar!(
            r#"SELECT id as "id!: unit::Id" FROM units WHERE name = ?1"#,
            name
        )
        .fetch_one(pool)
        .await?)
    }

    fn all_items() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by(field: Field, direction: Direction) -> OrderBy<Field> {
        OrderBy { field, direction }
    }

    async fn any_item(pool: &SqlitePool) -> Result<Item> {
        let list = list_id(pool, BUSIEST).await?;
        let mut page =
            Item::for_list(pool, list, all_items(), by(Field::Id, Direction::Ascending)).await?;
        Ok(page.items.swap_remove(0))
    }

    async fn count(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM items"#)
                .fetch_one(pool)
                .await?,
        )
    }

    fn ids(p: &OffsetPage<Item>) -> Vec<Id> {
        p.items.iter().map(|i| i.id).collect()
    }

    fn names(p: &OffsetPage<Item>) -> Vec<Name> {
        p.items.iter().map(|i| i.name.clone()).collect()
    }

    fn amounts(p: &OffsetPage<Item>) -> Vec<Amount> {
        p.items.iter().map(|i| i.amount).collect()
    }

    fn done_ats(p: &OffsetPage<Item>) -> Vec<Option<DoneAt>> {
        p.items.iter().map(|i| i.done_at).collect()
    }

    fn created_ats(p: &OffsetPage<Item>) -> Vec<CreatedAt> {
        p.items.iter().map(|i| i.created_at).collect()
    }

    /// Every `Some` sorted in `direction`, and every `None` after all of them.
    fn sorted_nulls_last<T: PartialOrd>(vals: &[Option<T>], direction: Direction) -> bool {
        let first_null = vals.iter().position(Option::is_none).unwrap_or(vals.len());
        if vals[first_null..].iter().any(Option::is_some) {
            return false;
        }
        let present = &vals[..first_null];

        match direction {
            Direction::Ascending => present.windows(2).all(|w| w[0] <= w[1]),
            Direction::Descending => present.windows(2).all(|w| w[0] >= w[1]),
        }
    }

    // ---------------------------------------------------------------- create

    #[rstest]
    #[case::plain("Apples", 1.0, Ok("Apples"))]
    #[case::trims_whitespace("  Apples  ", 1.0, Ok("Apples"))]
    // an item name is read back by a person, so its case is left alone
    #[case::keeps_case("Free-Range Eggs", 1.0, Ok("Free-Range Eggs"))]
    // amount is a REAL, and 1.5 kg is a thing people buy
    #[case::fractional_amount("Chicken thighs", 1.5, Ok("Chicken thighs"))]
    #[case::rejects_empty("", 1.0, Err(Error::InvalidInput))]
    #[case::rejects_whitespace_only("   ", 1.0, Err(Error::InvalidInput))]
    // `CHECK (amount > 0)`: none of a thing is not a line on a shopping list
    #[case::rejects_zero_amount("Apples", 0.0, Err(Error::InvalidInput))]
    #[case::rejects_negative_amount("Apples", -1.0, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] amount: f64,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;
        let kg = unit_id(&pool, "kg").await?;

        let got = Item::create(&pool, list, Name(input.into()), Amount(amount), Some(kg)).await;

        match (got, expected) {
            (Ok(item), Ok(want)) => {
                assert_eq!(item.name, Name(want.into()));
                assert_eq!(item.amount, Amount(amount));
                assert_eq!(item.list_id, list, "added to the list given");
                assert_eq!(item.unit_id, Some(kg));
                assert_eq!(item.done_at, None, "a new item is outstanding");
                assert_eq!(
                    Item::get(&pool, Lookup::Id(item.id)).await?,
                    item,
                    "the returned row is the one that was written"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(count(&pool).await?, 0, "a rejected item must not insert");
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    /// `items.unit_id` is nullable, and the fixture's `Cake candles` proves it. A
    /// `None` must survive the round trip as `None` rather than as a zero.
    #[rstest]
    #[tokio::test]
    async fn create_without_a_unit(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;

        let item =
            Item::create(&pool, list, Name("Birthday cake".into()), Amount(1.0), None).await?;

        assert_eq!(item.unit_id, None);
        assert_eq!(Item::get(&pool, Lookup::Id(item.id)).await?.unit_id, None);
        Ok(())
    }

    #[rstest]
    #[case::at_the_limit(MAX_NAME, Ok(()))]
    #[case::one_over_the_limit(MAX_NAME + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_the_name_length(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;

        let got = Item::create(&pool, list, Name("x".repeat(length)), Amount(1.0), None)
            .await
            .map(|_| ());

        assert_eq!(got, expected, "a name of {length} characters");
        Ok(())
    }

    /// Both references are the caller's to get right, and neither is the `InUse` kind
    /// of foreign-key failure.
    #[rstest]
    #[case::unknown_list(true, false)]
    #[case::unknown_unit(false, true)]
    #[tokio::test]
    async fn create_rejects_a_dangling_reference(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] bad_list: bool,
        #[case] bad_unit: bool,
    ) -> Result<()> {
        let list = if bad_list {
            list::Id(9999)
        } else {
            list_id(&pool, BUSIEST).await?
        };
        let unit = if bad_unit {
            Some(unit::Id(9999))
        } else {
            Some(unit_id(&pool, "kg").await?)
        };

        let result = Item::create(&pool, list, Name("orphan".into()), Amount(1.0), unit).await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(count(&pool).await?, 0);
        Ok(())
    }

    // ---------------------------------------------------------------- update

    #[rstest]
    #[tokio::test]
    async fn update_replaces_what_was_typed(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let before = any_item(&pool).await?;
        let g = unit_id(&pool, "g").await?;

        let after = Item::update(
            &pool,
            before.id,
            Name("  Braeburn apples ".into()),
            Amount(2.5),
            Some(g),
        )
        .await?;

        assert_eq!(after.id, before.id);
        assert_eq!(
            after.name,
            Name("Braeburn apples".into()),
            "trimmed, case kept"
        );
        assert_eq!(after.amount, Amount(2.5));
        assert_eq!(after.unit_id, Some(g));
        assert_eq!(after.list_id, before.list_id, "an edit is not a move");
        assert_eq!(
            after.done_at, before.done_at,
            "an edit does not tick it off"
        );
        assert_eq!(
            after.created_at, before.created_at,
            "editing must not restamp created_at"
        );
        Ok(())
    }

    #[rstest]
    #[case::rejects_whitespace_only("   ", 1.0)]
    #[case::rejects_zero_amount("Apples", 0.0)]
    #[tokio::test]
    async fn update_rejects_bad_input(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] name: &str,
        #[case] amount: f64,
    ) -> Result<()> {
        let before = any_item(&pool).await?;

        let result = Item::update(&pool, before.id, Name(name.into()), Amount(amount), None).await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(
            Item::get(&pool, Lookup::Id(before.id)).await?,
            before,
            "a rejected edit must leave the row alone"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn update_reports_a_miss(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = Item::update(&pool, Id(9999), Name("nothing".into()), Amount(1.0), None).await;

        assert!(
            matches!(result, Err(Error::NotFound)),
            "expected NotFound, got {result:?}"
        );
        assert_eq!(
            count(&pool).await?,
            SEEDED,
            "a missed update must not insert"
        );
        Ok(())
    }

    // ------------------------------------------------------------- done_at

    #[rstest]
    #[tokio::test]
    async fn set_done_ticks_off_and_back(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let item = any_item(&pool).await?;
        assert_eq!(item.done_at, None, "starts outstanding in the fixture");

        let done = Item::set_done(&pool, item.id, true).await?;
        assert!(done.done_at.is_some(), "ticking it off stamps done_at");
        assert!(
            done.done_at.unwrap().0 >= done.created_at.0,
            "done before it existed"
        );

        let undone = Item::set_done(&pool, item.id, false).await?;
        assert_eq!(undone.done_at, None, "putting it back clears the stamp");

        // nothing else moved
        assert_eq!(undone.name, item.name);
        assert_eq!(undone.amount, item.amount);
        assert_eq!(undone.unit_id, item.unit_id);
        assert_eq!(undone.created_at, item.created_at);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn set_done_reports_a_miss(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        assert!(matches!(
            Item::set_done(&pool, Id(9999), true).await,
            Err(Error::NotFound)
        ));
    }

    // ---------------------------------------------------------------- delete

    #[rstest]
    #[tokio::test]
    async fn delete(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
            "fixtures/tags.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let item = any_item(&pool).await?;
        let tags = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM item_tags WHERE item_id = ?1"#,
            item.id
        )
        .fetch_one(&pool)
        .await?;
        assert!(tags > 0, "need a tagged item to prove the cascade");

        Item::delete(&pool, item.id).await?;

        assert!(
            matches!(
                Item::get(&pool, Lookup::Id(item.id)).await,
                Err(Error::NotFound)
            ),
            "the row is gone"
        );
        assert_eq!(count(&pool).await?, SEEDED - 1);
        let left = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM item_tags WHERE item_id = ?1"#,
            item.id
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(left, 0, "its {tags} tag links went with it");

        let result = Item::delete(&pool, item.id).await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "deleting it twice reports the miss, got {result:?}"
        );
        Ok(())
    }

    // ---------------------------------------------------------------- lookup

    #[rstest]
    #[tokio::test]
    async fn get(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let want = any_item(&pool).await?;

        assert_eq!(Item::get(&pool, Lookup::Id(want.id)).await?, want);
        Ok(())
    }

    /// The fully sparse row: no unit, not done. Both must come back as `None`, not as
    /// a zero or an epoch.
    #[rstest]
    #[tokio::test]
    async fn get_reads_back_a_sparse_item(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let id = sqlx::query_scalar!(
            r#"SELECT id as "id!: Id" FROM items WHERE name = ?1"#,
            UNITLESS
        )
        .fetch_one(&pool)
        .await?;

        let item = Item::get(&pool, Lookup::Id(id)).await?;

        assert_eq!(item.unit_id, None, "{UNITLESS} has no unit");
        assert_eq!(item.done_at, None);
        Ok(())
    }

    #[rstest]
    #[case::missing_id(Lookup::Id(Id(9999)))]
    #[case::zero_id(Lookup::Id(Id(0)))]
    #[tokio::test]
    async fn get_reports_a_miss(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] lookup: Lookup,
    ) {
        assert!(matches!(
            Item::get(&pool, lookup).await,
            Err(Error::NotFound)
        ));
    }

    // --------------------------------------------------------------- scoping

    #[rstest]
    #[tokio::test]
    async fn for_list_returns_only_that_lists_items(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;

        let page = Item::for_list(
            &pool,
            list,
            all_items(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert_eq!(page.total, BUSIEST_ITEMS, "counts only this list's items");
        assert_eq!(page.items.len(), BUSIEST_ITEMS as usize);
        assert!(
            page.items.iter().all(|i| i.list_id == list),
            "an item from another list leaked into the page"
        );
        assert!(
            count(&pool).await? > BUSIEST_ITEMS,
            "other items exist to leak"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn for_list_is_empty_for_a_list_with_nothing_on_it(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        // the fixture's spare lists carry no items
        let empty = list_id(&pool, "Chemist").await?;

        let page = Item::for_list(
            &pool,
            empty,
            all_items(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert!(page.items.is_empty());
        assert_eq!(page.total, 0, "the total is this list's, not the table's");
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_more);
        Ok(())
    }

    // -------------------------------------------------------------- ordering

    struct OrderCase {
        order_by: OrderBy<Field>,
        assert: fn(&OffsetPage<Item>),
    }

    #[rstest]
    #[case::id_ascending(OrderCase {
        order_by: OrderBy { field: Field::Id, direction: Direction::Ascending },
        assert: |p| assert!(ids(p).windows(2).all(|w| w[0].0 < w[1].0), "{:?}", ids(p)),
    })]
    #[case::id_descending(OrderCase {
        order_by: OrderBy { field: Field::Id, direction: Direction::Descending },
        assert: |p| assert!(ids(p).windows(2).all(|w| w[0].0 > w[1].0), "{:?}", ids(p)),
    })]
    #[case::name_ascending(OrderCase {
        order_by: OrderBy { field: Field::Name, direction: Direction::Ascending },
        assert: |p| assert!(names(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", names(p)),
    })]
    #[case::name_descending(OrderCase {
        order_by: OrderBy { field: Field::Name, direction: Direction::Descending },
        assert: |p| assert!(names(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", names(p)),
    })]
    #[case::amount_ascending(OrderCase {
        order_by: OrderBy { field: Field::Amount, direction: Direction::Ascending },
        assert: |p| assert!(amounts(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", amounts(p)),
    })]
    #[case::amount_descending(OrderCase {
        order_by: OrderBy { field: Field::Amount, direction: Direction::Descending },
        assert: |p| assert!(amounts(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", amounts(p)),
    })]
    #[case::done_at_ascending_nulls_last(OrderCase {
        order_by: OrderBy { field: Field::DoneAt, direction: Direction::Ascending },
        assert: |p| {
            assert!(sorted_nulls_last(&done_ats(p), Direction::Ascending), "{:?}", done_ats(p));
            assert_eq!(done_ats(p).last(), Some(&None), "outstanding items sort last");
        },
    })]
    #[case::done_at_descending_nulls_last(OrderCase {
        order_by: OrderBy { field: Field::DoneAt, direction: Direction::Descending },
        assert: |p| {
            assert!(sorted_nulls_last(&done_ats(p), Direction::Descending), "{:?}", done_ats(p));
            // NULLS LAST applies to both branches, not just the ascending one
            assert_eq!(done_ats(p).last(), Some(&None));
        },
    })]
    #[case::created_at_ascending(OrderCase {
        order_by: OrderBy { field: Field::CreatedAt, direction: Direction::Ascending },
        assert: |p| assert!(created_ats(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", created_ats(p)),
    })]
    #[case::created_at_descending(OrderCase {
        order_by: OrderBy { field: Field::CreatedAt, direction: Direction::Descending },
        assert: |p| assert!(created_ats(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", created_ats(p)),
    })]
    #[tokio::test]
    async fn for_list_orders_by_every_field(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: OrderCase,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;

        let page = Item::for_list(&pool, list, all_items(), c.order_by).await?;
        assert_eq!(page.items.len(), BUSIEST_ITEMS as usize);
        (c.assert)(&page);
        Ok(())
    }

    /// Each field must produce a *different* order. A [`Field`] variant with no
    /// matching arm in the SQL `CASE` falls through to NULL for every row, which
    /// orders nothing and raises no error — this is what catches that.
    ///
    /// It leans on the fixture keeping this list's amounts, names, stamps and done
    /// flags out of step with each other.
    #[rstest]
    #[tokio::test]
    async fn for_list_every_field_changes_the_order(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;

        let mut orders = Vec::new();
        for &field in Field::VARIANTS {
            for direction in [Direction::Ascending, Direction::Descending] {
                let page = Item::for_list(&pool, list, all_items(), by(field, direction)).await?;
                orders.push((format!("{field:?} {direction:?}"), ids(&page)));
            }
        }

        for (i, (left_name, left)) in orders.iter().enumerate() {
            for (right_name, right) in orders.iter().skip(i + 1) {
                assert_ne!(
                    left, right,
                    "{left_name} and {right_name} returned the same order, so at least \
                     one of them is not ordering at all"
                );
            }
        }
        Ok(())
    }

    // --------------------------------------------------------------- paging

    struct PageCase {
        page: Paging,
        items: usize,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_page(
        PageCase { page: Paging { number: 1, size: 4 }, items: 4, total_pages: 3, has_more: true }
    )]
    #[case::middle_page(
        PageCase { page: Paging { number: 2, size: 4 }, items: 4, total_pages: 3, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { page: Paging { number: 3, size: 4 }, items: 1, total_pages: 3, has_more: false }
    )]
    #[case::page_larger_than_the_list(
        PageCase { page: Paging { number: 1, size: 100 }, items: BUSIEST_ITEMS as usize, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { page: Paging { number: 99, size: 4 }, items: 0, total_pages: 3, has_more: false }
    )]
    // a negative LIMIT means "no limit" to SQLite; Paging::limit clamps it so that a
    // bad page size cannot dump the whole list
    #[case::negative_size_is_empty(
        PageCase { page: Paging { number: 1, size: -1 }, items: 0, total_pages: 0, has_more: true }
    )]
    // offset would overflow i64 and panic in debug without the saturating multiply
    #[case::huge_page_number(
        PageCase { page: Paging { number: i64::MAX, size: 4 }, items: 0, total_pages: 3, has_more: false }
    )]
    #[tokio::test]
    async fn for_list_pages(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: PageCase,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;

        let page = Item::for_list(&pool, list, c.page, by(Field::Id, Direction::Ascending)).await?;

        assert_eq!(page.items.len(), c.items, "items on the page");
        assert_eq!(
            page.total, BUSIEST_ITEMS,
            "total is independent of the page"
        );
        assert_eq!(page.total_pages, c.total_pages);
        assert_eq!(page.has_more, c.has_more);
        Ok(())
    }

    #[rstest]
    #[case::by_id(Field::Id)]
    #[case::by_name(Field::Name)]
    #[case::by_amount(Field::Amount)]
    #[case::by_done_at(Field::DoneAt)]
    #[case::by_created_at(Field::CreatedAt)]
    #[tokio::test]
    async fn for_list_walks_every_item_exactly_once(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] field: Field,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;
        let mut seen = Vec::new();
        let mut number = 1;

        loop {
            let page = Item::for_list(
                &pool,
                list,
                Paging { number, size: 4 },
                by(field, Direction::Ascending),
            )
            .await?;
            seen.extend(ids(&page));
            if !page.has_more {
                assert_eq!(
                    page.total_pages, number,
                    "has_more cleared on the last page"
                );
                break;
            }
            number += 1;
            assert!(number < 100, "has_more never cleared");
        }

        let mut unique = seen.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(seen.len(), BUSIEST_ITEMS as usize, "paged over {seen:?}");
        assert_eq!(unique.len(), BUSIEST_ITEMS as usize, "repeated an item");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn for_list_totals_ignore_other_lists(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list = list_id(&pool, BUSIEST).await?;
        let other = list_id(&pool, "Dairy").await?;
        let order = by(Field::Id, Direction::Ascending);

        let before = Item::for_list(&pool, list, all_items(), order).await?;
        assert_eq!(before.total, BUSIEST_ITEMS);

        Item::create(&pool, other, Name("not here".into()), Amount(1.0), None).await?;
        let after = Item::for_list(&pool, list, all_items(), order).await?;
        assert_eq!(
            after.total, BUSIEST_ITEMS,
            "another list's item changed nothing"
        );

        Item::create(&pool, list, Name("here".into()), Amount(1.0), None).await?;
        let mine = Item::for_list(&pool, list, all_items(), order).await?;
        assert_eq!(mine.total, BUSIEST_ITEMS + 1);
        Ok(())
    }
}
