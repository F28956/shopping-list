use time::OffsetDateTime;

use super::{Error, Result};
use super::{OffsetPage, OrderBy, Paging};

// Scaffold Id, Name and CreatedAt
i64!(Id);
string!(Name);
timestamp!(CreatedAt);

// Unit names are stored trimmed and lowercased, so `Kg`, `kg ` and `KG` are one unit
normalized!(Name);

/// The longest name `units.name` accepts, in characters — keep in step with the
/// `CHECK` in the init migration. Anything longer is [`Error::InvalidInput`].
pub const MAX_NAME: usize = 64;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Unit {
    pub id: Id,
    pub name: Name,
    pub created_at: CreatedAt,
}

/// How a caller asks for a single unit. Every variant must be able to identify at
/// most one row, which is why there is no `CreatedAt` here: it is orderable but not
/// a key. Keeping it out means `get` cannot be handed an argument it has to reject
/// at runtime.
#[derive(Debug, Clone)]
pub enum Lookup {
    Id(Id),
    Name(Name),
}

/// What `list` may order by. Deliberately a separate enum from [`Lookup`] — the set
/// of sortable columns and the set of unique keys are not the same set.
///
/// Every variant added here needs a matching `WHEN` arm in both `CASE` branches of
/// the `list` query. A variant without one silently sorts by nothing, so
/// `list_every_field_changes_the_order` exists to fail the build's tests when the
/// two drift apart.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Id,
    Name,
    CreatedAt,
}

impl Unit {
    /// Inserts a unit under its [normalised](Name::normalized) name.
    ///
    /// A name that collides with an existing one is [`Error::Conflict`]; one that
    /// normalises to empty is [`Error::InvalidInput`].
    pub async fn create(pool: &sqlx::SqlitePool, name: Name) -> Result<Unit> {
        let name = name.normalized();

        let unit = sqlx::query_as!(
            Unit,
            r#"
            INSERT INTO units (name)
            VALUES (?1)
            RETURNING
                id          as "id: Id",
                name        as "name: Name",
                created_at  as "created_at: CreatedAt"
            "#,
            name,
        )
        .fetch_one(pool)
        .await?;

        Ok(unit)
    }

    /// Renames a unit. `RETURNING` with `fetch_one` means an id that matches no row
    /// is [`Error::NotFound`] without a second query.
    pub async fn update(pool: &sqlx::SqlitePool, id: Id, name: Name) -> Result<Unit> {
        let name = name.normalized();

        let unit = sqlx::query_as!(
            Unit,
            r#"
            UPDATE units SET name = ?1 WHERE id = ?2
            RETURNING
                id          as "id: Id",
                name        as "name: Name",
                created_at  as "created_at: CreatedAt"
            "#,
            name,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(unit)
    }

    /// Deletes a unit.
    ///
    /// `items.unit_id` is `ON DELETE RESTRICT`, so deleting a unit an item still
    /// points at is [`Error::InUse`], not a server fault.
    pub async fn delete(pool: &sqlx::SqlitePool, id: Id) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM units WHERE id = ?1
            "#,
            id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Fetches one page of units. The total is a second `count(*)`, which SQLite
    /// answers from the smallest index and is cheap at this scale.
    ///
    /// The two statements are not in a transaction, so a concurrent write between
    /// them can leave `total` describing a table the page does not. Folding the count
    /// into the page query as `count(*) OVER ()` would fix that but lose the total
    /// whenever a page is empty — including every page past the end — so the second
    /// query is the deliberate choice.
    pub async fn list(
        pool: &sqlx::SqlitePool,
        page: Paging,
        order_by: OrderBy<Field>,
    ) -> Result<OffsetPage<Unit>> {
        let field: &'static str = order_by.field.into();
        let direction: &'static str = order_by.direction.into();

        let limit = page.limit();
        let offset = page.offset();

        let units = sqlx::query_as!(
            Unit,
            r#"
        SELECT
            id          as "id: Id",
            name        as "name: Name",
            created_at  as "created_at: CreatedAt"
        FROM units
        ORDER BY
            CASE
                WHEN ?2 = 'ascending' THEN
                    CASE ?1
                        WHEN 'id' THEN id
                        -- NOCASE is a backstop only. Names are normalised in Rust
                        -- before they are written, and nothing in the schema enforces
                        -- that, so a row written by anything other than this model
                        -- (a fixture, a migration, a hand-run statement) may not be.
                        WHEN 'name' THEN name COLLATE NOCASE
                        WHEN 'created_at' THEN created_at
                    END
                END ASC NULLS LAST,
            CASE
                WHEN ?2 = 'descending' THEN
                    CASE ?1
                        WHEN 'id' THEN id
                        WHEN 'name' THEN name COLLATE NOCASE
                        WHEN 'created_at' THEN created_at
                    END
            END DESC NULLS LAST,
            -- keeps paging deterministic when the sort key ties
            id ASC
        LIMIT ?3 OFFSET ?4
        "#,
            field,
            direction,
            limit,
            offset,
        )
        .fetch_all(pool)
        .await?;

        let total = sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM units"#)
            .fetch_one(pool)
            .await?;

        Ok(page.page_of(units, total))
    }

    /// Fetches one unit.
    ///
    /// Names are [normalised](Name::normalized) before matching, so a lookup finds the row whatever
    /// case the caller had it in. A miss is [`Error::NotFound`], not `Ok(None)`.
    pub async fn get(pool: &sqlx::SqlitePool, by: Lookup) -> Result<Unit> {
        let unit = match by {
            Lookup::Id(v) => {
                sqlx::query_as!(
                    Unit,
                    r#"
                SELECT
                    id          as "id: Id",
                    name        as "name: Name",
                    created_at  as "created_at: CreatedAt"
                FROM units
                WHERE id = ?1 "#,
                    v
                )
                .fetch_one(pool)
                .await?
            }
            Lookup::Name(v) => {
                let name = v.normalized();
                sqlx::query_as!(
                    Unit,
                    r#"
                SELECT
                    id          as "id: Id",
                    name        as "name: Name",
                    created_at  as "created_at: CreatedAt"
                FROM units
                WHERE name = ?1 "#,
                    name
                )
                .fetch_one(pool)
                .await?
            }
        };
        Ok(unit)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;
    use time::Duration;

    use strum::VariantArray;

    use super::*;
    use crate::models::{Direction, pool};

    /// Units in `fixtures/units.sql`.
    const SEEDED: i64 = 31;

    fn all_units() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by(field: Field, direction: Direction) -> OrderBy<Field> {
        OrderBy { field, direction }
    }

    async fn any_unit(pool: &SqlitePool) -> Result<Unit> {
        let mut page = Unit::list(pool, all_units(), by(Field::Id, Direction::Ascending)).await?;
        Ok(page.items.swap_remove(0))
    }

    async fn count(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM units"#)
                .fetch_one(pool)
                .await?,
        )
    }

    fn ids(p: &OffsetPage<Unit>) -> Vec<Id> {
        p.items.iter().map(|u| u.id).collect()
    }

    fn names(p: &OffsetPage<Unit>) -> Vec<Name> {
        p.items.iter().map(|u| u.name.clone()).collect()
    }

    fn created_ats(p: &OffsetPage<Unit>) -> Vec<CreatedAt> {
        p.items.iter().map(|u| u.created_at).collect()
    }

    // ---------------------------------------------------------------- create

    #[rstest]
    #[case::plain("kg", Ok("kg"))]
    #[case::multi_word("fl oz", Ok("fl oz"))]
    #[case::trims_whitespace("    kg  ", Ok("kg"))]
    #[case::lowercases("KiloGram", Ok("kilogram"))]
    // SQLite's lower() folds ASCII only, so this is stored unchanged if the model
    // normalises in SQL rather than in Rust
    #[case::lowercases_non_ascii("Ångström", Ok("ångström"))]
    #[case::rejects_empty("", Err(Error::InvalidInput))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    // str::trim strips every Unicode space; SQLite's trim() only strips U+0020
    #[case::rejects_non_breaking_space_only("\u{00A0}", Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create(
        #[future(awt)] pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let got = Unit::create(&pool, Name(input.into())).await;

        match (got, expected) {
            (Ok(unit), Ok(want)) => {
                assert_eq!(unit.name, Name(want.into()), "stored under its name");
                let age = OffsetDateTime::now_utc() - unit.created_at.0;
                assert!(
                    (Duration::ZERO..Duration::minutes(1)).contains(&age),
                    "created_at should be stamped now, was {age} ago"
                );
                assert_eq!(
                    Unit::get(&pool, Lookup::Id(unit.id)).await?,
                    unit,
                    "the returned row is the one that was written"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(count(&pool).await?, 0, "a rejected name must not insert");
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    /// Names are bounded, so a caller cannot park a megabyte in the table.
    #[rstest]
    #[case::at_the_limit(MAX_NAME, Ok(()))]
    #[case::one_over_the_limit(MAX_NAME + 1, Err(Error::InvalidInput))]
    #[case::absurd(100_000, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_the_name_length(
        #[future(awt)] pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let name = Name("x".repeat(length));

        let got = Unit::create(&pool, name).await.map(|_| ());

        assert_eq!(got, expected, "a name of {length} characters");
        Ok(())
    }

    /// The bound applies to renames too, not just inserts.
    #[rstest]
    #[case::at_the_limit(MAX_NAME, Ok(()))]
    #[case::one_over_the_limit(MAX_NAME + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn update_bounds_the_name_length(
        #[future(awt)] pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let unit = Unit::create(&pool, Name("my-unit".into())).await?;

        let got = Unit::update(&pool, unit.id, Name("x".repeat(length)))
            .await
            .map(|_| ());

        assert_eq!(got, expected, "a name of {length} characters");
        Ok(())
    }

    /// `units.name` is `UNIQUE`, and every name is normalised before it is stored, so
    /// a second unit collides whenever it differs only in case or padding.
    #[rstest]
    #[case::exact_duplicate("kilogram")]
    #[case::differs_only_in_case("KiloGram")]
    #[case::differs_only_in_padding("  kilogram ")]
    #[tokio::test]
    async fn create_rejects_duplicate_name(
        #[future(awt)] pool: SqlitePool,
        #[case] duplicate: &str,
    ) -> Result<()> {
        Unit::create(&pool, Name("kilogram".into())).await?;

        let err = Unit::create(&pool, Name(duplicate.into()))
            .await
            .expect_err("duplicate name must not insert");

        assert_eq!(err, Error::Conflict);
        assert_eq!(
            count(&pool).await?,
            1,
            "the failed insert must not add a row"
        );
        Ok(())
    }

    /// The non-ASCII half of the same rule. `COLLATE NOCASE` does not fold `Å`, so
    /// this only holds because the name is lowercased in Rust before it is stored.
    #[rstest]
    #[tokio::test]
    async fn create_rejects_duplicate_name_non_ascii(
        #[future(awt)] pool: SqlitePool,
    ) -> Result<()> {
        Unit::create(&pool, Name("ångström".into())).await?;

        let err = Unit::create(&pool, Name("Ångström".into()))
            .await
            .expect_err("duplicate name must not insert");

        assert_eq!(err, Error::Conflict);
        assert_eq!(
            count(&pool).await?,
            1,
            "the failed insert must not add a row"
        );
        Ok(())
    }

    // ---------------------------------------------------------------- update

    #[rstest]
    #[case::renames("my-renamed-unit", Ok("my-renamed-unit"))]
    #[case::to_the_same_name("my-unit", Ok("my-unit"))]
    #[case::trims_whitespace(" my-renamed-unit     ", Ok("my-renamed-unit"))]
    #[case::lowercases(" My-Renamed-Unit     ", Ok("my-renamed-unit"))]
    #[case::lowercases_non_ascii("Ångström", Ok("ångström"))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    #[case::rejects_a_name_already_taken("kg", Err(Error::Conflict))]
    #[case::rejects_a_taken_name_in_another_case("KG", Err(Error::Conflict))]
    #[tokio::test]
    async fn update(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let unit = Unit::create(&pool, Name("my-unit".into())).await?;

        let result = Unit::update(&pool, unit.id, Name(input.into())).await;

        match (result, expected) {
            (Ok(renamed), Ok(want)) => {
                assert_eq!(renamed.id, unit.id, "renaming must not move the row");
                assert_eq!(renamed.name, Name(want.into()));
                assert_eq!(
                    renamed.created_at, unit.created_at,
                    "renaming must not restamp created_at"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(
                    Unit::get(&pool, Lookup::Id(unit.id)).await?.name,
                    unit.name,
                    "a rejected rename must leave the row alone"
                );
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn update_reports_a_miss(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = Unit::update(&pool, Id(9999), Name("nothing".into())).await;

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

    // ---------------------------------------------------------------- delete

    #[rstest]
    #[case::kilogram("kg")]
    #[case::pound("pound")]
    #[tokio::test]
    async fn delete(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] name: &str,
    ) -> Result<()> {
        let unit = Unit::get(&pool, Lookup::Name(Name(name.into()))).await?;

        Unit::delete(&pool, unit.id).await?;

        assert!(
            matches!(
                Unit::get(&pool, Lookup::Id(unit.id)).await,
                Err(Error::NotFound)
            ),
            "the row is gone"
        );
        assert_eq!(count(&pool).await?, SEEDED - 1);

        let result = Unit::delete(&pool, unit.id).await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "deleting it twice reports the miss, got {result:?}"
        );
        Ok(())
    }

    /// `items.unit_id` is `ON DELETE RESTRICT`. SQLite implements that with an
    /// internal trigger, so the blocked delete arrives as SQLITE_CONSTRAINT_TRIGGER
    /// rather than the foreign-key code sqlx knows about — see `models::error`.
    #[rstest]
    #[case::in_use_by_an_item("kg")]
    #[tokio::test]
    async fn delete_reports_a_unit_still_in_use(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] name: &str,
    ) -> Result<()> {
        let unit = Unit::get(&pool, Lookup::Name(Name(name.into()))).await?;

        let result = Unit::delete(&pool, unit.id).await;

        assert!(
            matches!(&result, Err(Error::InUse)),
            "expected InUse, got {result:?}"
        );
        assert_eq!(
            Unit::get(&pool, Lookup::Id(unit.id)).await?,
            unit,
            "a blocked delete must leave the row alone"
        );
        Ok(())
    }

    // ---------------------------------------------------------------- lookup

    #[rstest]
    #[tokio::test]
    async fn get(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let want = any_unit(&pool).await?;

        assert_eq!(Unit::get(&pool, Lookup::Id(want.id)).await?, want);
        assert_eq!(
            Unit::get(&pool, Lookup::Name(want.name.clone())).await?,
            want
        );
        Ok(())
    }

    /// Callers do not have to know the stored form of a name to look one up.
    #[rstest]
    #[case::shouted("KG")]
    #[case::mixed_case("Kg")]
    #[case::padded("  kg  ")]
    #[tokio::test]
    async fn get_by_name_normalises_the_lookup(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
    ) -> Result<()> {
        let want = Unit::get(&pool, Lookup::Name(Name("kg".into()))).await?;

        assert_eq!(
            Unit::get(&pool, Lookup::Name(Name(input.into()))).await?,
            want
        );
        Ok(())
    }

    /// The non-ASCII half: `COLLATE NOCASE` cannot fold `Å`, so this passes only
    /// because the lookup is normalised in Rust.
    #[rstest]
    #[tokio::test]
    async fn get_by_name_normalises_non_ascii(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let want = Unit::create(&pool, Name("ångström".into())).await?;

        assert_eq!(
            Unit::get(&pool, Lookup::Name(Name("Ångström".into()))).await?,
            want
        );
        Ok(())
    }

    /// `fetch_one` means a miss is an error, not `Ok(None)`. Every caller has to
    /// handle that, so it is worth stating.
    #[rstest]
    #[case::missing_id(Lookup::Id(Id(9999)))]
    #[case::zero_id(Lookup::Id(Id(0)))]
    #[case::unknown_name(Lookup::Name(Name("nonesuch".into())))]
    #[case::empty_name(Lookup::Name(Name("".into())))]
    #[tokio::test]
    async fn get_reports_a_miss(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] lookup: Lookup,
    ) {
        assert!(matches!(
            Unit::get(&pool, lookup).await,
            Err(Error::NotFound)
        ));
    }

    // -------------------------------------------------------------- ordering

    struct OrderCase {
        order_by: OrderBy<Field>,
        assert: fn(&OffsetPage<Unit>),
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
    #[case::created_at_ascending(OrderCase {
        order_by: OrderBy { field: Field::CreatedAt, direction: Direction::Ascending },
        assert: |p| assert!(created_ats(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", created_ats(p)),
    })]
    #[case::created_at_descending(OrderCase {
        order_by: OrderBy { field: Field::CreatedAt, direction: Direction::Descending },
        assert: |p| assert!(created_ats(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", created_ats(p)),
    })]
    #[tokio::test]
    async fn list_orders_by_every_field(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: OrderCase,
    ) -> Result<()> {
        let page = Unit::list(&pool, all_units(), c.order_by).await?;
        assert_eq!(page.items.len(), SEEDED as usize);
        (c.assert)(&page);
        Ok(())
    }

    /// Each field must produce a *different* order. A [`Field`] variant with no
    /// matching arm in the SQL `CASE` falls through to NULL for every row, which
    /// orders nothing and raises no error — this is what catches that.
    ///
    /// It works because `fixtures/units.sql` stamps `created_at` deliberately out of
    /// id order; with the column default every row would share a timestamp and this
    /// would be a false negative.
    #[rstest]
    #[tokio::test]
    async fn list_every_field_changes_the_order(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let mut orders = Vec::new();
        for &field in Field::VARIANTS {
            for direction in [Direction::Ascending, Direction::Descending] {
                let page = Unit::list(&pool, all_units(), by(field, direction)).await?;
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
        units: usize,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_page(
        PageCase { page: Paging { number: 1, size: 6 }, units: 6, total_pages: 6, has_more: true }
    )]
    #[case::middle_page(
        PageCase { page: Paging { number: 3, size: 6 }, units: 6, total_pages: 6, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { page: Paging { number: 6, size: 6 }, units: 1, total_pages: 6, has_more: false }
    )]
    #[case::page_larger_than_the_table(
        PageCase { page: Paging { number: 1, size: 100 }, units: SEEDED as usize, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { page: Paging { number: 99, size: 6 }, units: 0, total_pages: 6, has_more: false }
    )]
    // a negative LIMIT means "no limit" to SQLite; Paging::limit clamps it so that a
    // bad page size cannot dump the whole table
    #[case::negative_size_is_empty(
        PageCase { page: Paging { number: 1, size: -1 }, units: 0, total_pages: 0, has_more: true }
    )]
    // offset would overflow i64 and panic in debug without the saturating multiply
    #[case::huge_page_number(
        PageCase { page: Paging { number: i64::MAX, size: 6 }, units: 0, total_pages: 6, has_more: false }
    )]
    #[tokio::test]
    async fn list_pages(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: PageCase,
    ) -> Result<()> {
        let page = Unit::list(&pool, c.page, by(Field::Id, Direction::Ascending)).await?;

        assert_eq!(page.items.len(), c.units, "units on the page");
        assert_eq!(page.total, SEEDED, "total is independent of the page");
        assert_eq!(page.total_pages, c.total_pages);
        assert_eq!(page.has_more, c.has_more);
        Ok(())
    }

    /// Walking `has_more` must reach every unit exactly once. A swapped `LIMIT`/
    /// `OFFSET` binding still returns plausible pages, but not this.
    #[rstest]
    #[case::by_id(Field::Id)]
    #[case::by_name(Field::Name)]
    #[case::by_created_at(Field::CreatedAt)]
    #[tokio::test]
    async fn list_walks_every_unit_exactly_once(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] field: Field,
    ) -> Result<()> {
        let mut seen = Vec::new();
        let mut number = 1;

        loop {
            let page = Unit::list(
                &pool,
                Paging { number, size: 6 },
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
        assert_eq!(seen.len(), SEEDED as usize, "paged over {seen:?}");
        assert_eq!(unique.len(), SEEDED as usize, "repeated a unit");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_totals_track_inserts(
        #[with(seeds!("fixtures/units.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        // a size whose last page is partial, so one insert fills it rather than
        // spilling onto a new one
        const SIZE: i64 = 8;
        let pages = (SEEDED + SIZE - 1) / SIZE;
        let on_last = SEEDED - (pages - 1) * SIZE;
        assert!(on_last < SIZE, "SEEDED must not divide evenly into SIZE");

        let page = |n| Paging {
            number: n,
            size: SIZE,
        };
        let order = by(Field::Id, Direction::Ascending);

        let before = Unit::list(&pool, page(pages), order).await?;
        assert_eq!(before.total, SEEDED);
        assert_eq!(before.total_pages, pages);
        assert_eq!(before.items.len(), on_last as usize);
        assert!(!before.has_more);

        Unit::create(&pool, Name("my-unit".into())).await?;

        let after = Unit::list(&pool, page(pages), order).await?;
        assert_eq!(after.total, SEEDED + 1);
        assert_eq!(after.total_pages, pages, "the new unit fits the last page");
        assert_eq!(after.items.len(), on_last as usize + 1);
        assert!(!after.has_more);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_on_an_empty_table(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let page = Unit::list(
            &pool,
            Paging {
                number: 1,
                size: 10,
            },
            by(Field::Name, Direction::Ascending),
        )
        .await?;

        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_more);
        Ok(())
    }
}
