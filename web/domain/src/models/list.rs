use time::OffsetDateTime;

use super::user;
use super::{Error, Result};
use super::{OffsetPage, OrderBy, Paging};

// Scaffold Id, Name, CreatedAt and UpdatedAt
i64!(Id);
string!(Name);
timestamp!(CreatedAt);
timestamp!(UpdatedAt);

// A list name is free text a person reads back, so only the padding comes off
trimmed!(Name);

/// The longest name `lists.name` accepts, in characters — keep in step with the
/// `CHECK` in the init migration. Anything longer is [`Error::InvalidInput`].
pub const MAX_NAME: usize = 128;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct List {
    pub id: Id,
    pub name: Name,
    pub owner_id: user::Id,
    pub created_at: CreatedAt,
    pub updated_at: UpdatedAt,
}

/// How a caller asks for a single list. Only `id` identifies one: `lists.name` is not
/// unique, not even within an owner, so two of a person's lists may share a name.
#[derive(Debug, Clone)]
pub enum Lookup {
    Id(Id),
}

/// What `for_user` may order by. Deliberately a separate enum from [`Lookup`] — the
/// set of sortable columns and the set of unique keys are not the same set.
///
/// `owner_id` is absent on purpose: `for_user` scopes to one owner, so within a page
/// it is a constant and ordering by it would do nothing.
///
/// Every variant added here needs a matching `WHEN` arm in both `CASE` branches of
/// the query. A variant without one silently sorts by nothing, which is what
/// `visible_to_every_field_changes_the_order` exists to catch.
/// The default is `UpdatedAt`: the one touched most recently is the one being shopped.
///
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Id,
    Name,
    CreatedAt,
    #[default]
    UpdatedAt,
}

/// A member's standing on a list.
///
/// Ordered, so a required role can be compared against a held one: `Viewer < Editor <
/// Owner`. Everything a viewer may do, an editor may do; everything an editor may do,
/// an owner may do. Reading a permission check is then just `held >= needed`.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    sqlx::Type,
)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Read, and nothing else.
    Viewer,
    /// Everything on the list itself: add, tick, edit, tag, remove.
    Editor,
    /// The list too: rename it, delete it, and decide who else is on it.
    ///
    /// Held by exactly one person — `lists.owner_id` — and never granted by an
    /// invitation. There is no transfer: an owner who wants out deletes the list.
    Owner,
}

/// A row of `list_members`.
///
/// The owner is deliberately *not* one of these: `lists.owner_id` is the single
/// source of truth for ownership, so the two can never disagree. Membership is
/// purely additive.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct ListMember {
    pub list_id: Id,
    pub user_id: user::Id,
    pub role: Role,
    pub added_at: CreatedAt,
}

impl ListMember {
    /// The role this person holds on this list, if any.
    ///
    /// Ownership is answered from `lists.owner_id` rather than from a membership row,
    /// so a member row can never contradict who owns the list.
    pub async fn role_of(
        pool: &sqlx::SqlitePool,
        list_id: Id,
        user_id: user::Id,
    ) -> Result<Option<Role>> {
        if let Some(owner) = sqlx::query_scalar!(
            r#"SELECT owner_id as "owner_id: user::Id" FROM lists WHERE id = ?1"#,
            list_id
        )
        .fetch_optional(pool)
        .await?
            && owner == user_id
        {
            return Ok(Some(Role::Owner));
        }

        Ok(sqlx::query_scalar!(
            r#"SELECT role as "role: Role" FROM list_members WHERE list_id = ?1 AND user_id = ?2"#,
            list_id,
            user_id
        )
        .fetch_optional(pool)
        .await?)
    }

    /// How many people each of this person's lists is shared with.
    ///
    /// One query for the whole index rather than one per row: a lists page that asks
    /// per list turns ten lists into eleven round trips, and the answer is a single
    /// group-by.
    pub async fn counts_for(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
    ) -> Result<std::collections::HashMap<i64, i64>> {
        let rows = sqlx::query!(
            r#"
            SELECT m.list_id as "list_id!: i64", count(*) as "n!: i64"
            FROM list_members m
            JOIN lists l ON l.id = m.list_id
            WHERE l.owner_id = ?1
               OR m.list_id IN (SELECT list_id FROM list_members WHERE user_id = ?1)
            GROUP BY m.list_id
            "#,
            user_id
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.into_iter().map(|r| (r.list_id, r.n)).collect())
    }

    /// Everyone sharing a list, most recently added first. The owner is not among
    /// them — see the note on the struct.
    pub async fn for_list(pool: &sqlx::SqlitePool, list_id: Id) -> Result<Vec<ListMember>> {
        Ok(sqlx::query_as!(
            ListMember,
            r#"
            SELECT
                list_id  as "list_id: Id",
                user_id  as "user_id: user::Id",
                role     as "role: Role",
                added_at as "added_at: CreatedAt"
            FROM list_members
            WHERE list_id = ?1
            ORDER BY added_at DESC
            "#,
            list_id
        )
        .fetch_all(pool)
        .await?)
    }

    /// Adds someone, or changes the role they already hold.
    ///
    /// Idempotent, because redeeming the same invitation twice is a double-click, not
    /// an error worth showing anybody.
    pub async fn put(
        pool: &sqlx::SqlitePool,
        list_id: Id,
        user_id: user::Id,
        role: Role,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO list_members (list_id, user_id, role)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(list_id, user_id) DO UPDATE SET role = ?3
            "#,
            list_id,
            user_id,
            role
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Removes someone from a list. A miss is [`Error::NotFound`].
    pub async fn remove(pool: &sqlx::SqlitePool, list_id: Id, user_id: user::Id) -> Result<()> {
        let result = sqlx::query!(
            r#"DELETE FROM list_members WHERE list_id = ?1 AND user_id = ?2"#,
            list_id,
            user_id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }
}

impl List {
    /// Starts a list for a user.
    ///
    /// An `owner_id` that matches nobody is [`Error::InvalidInput`] — the reference
    /// is the caller's mistake, not a server fault.
    pub async fn create(pool: &sqlx::SqlitePool, owner_id: user::Id, name: Name) -> Result<List> {
        let name = name.trimmed();

        let list = sqlx::query_as!(
            List,
            r#"
            INSERT INTO lists (name, owner_id)
            VALUES (?1, ?2)
            RETURNING
                id          as "id!: Id",
                name        as "name: Name",
                owner_id    as "owner_id: user::Id",
                created_at  as "created_at!: CreatedAt",
                updated_at  as "updated_at!: UpdatedAt"
            "#,
            name,
            owner_id,
        )
        .fetch_one(pool)
        .await?;

        Ok(list)
    }

    /// Renames a list and stamps `updated_at`.
    ///
    /// The stamp is set here rather than by a trigger so that it is visible in the
    /// same place as the write it describes. `owner_id` is not writable: handing a
    /// list to someone else is a transfer, which is not the same operation as a
    /// rename and would need its own checks.
    pub async fn update(pool: &sqlx::SqlitePool, id: Id, name: Name) -> Result<List> {
        let name = name.trimmed();

        let list = sqlx::query_as!(
            List,
            r#"
            UPDATE lists SET name = ?1, updated_at = unixepoch() WHERE id = ?2
            RETURNING
                id          as "id!: Id",
                name        as "name: Name",
                owner_id    as "owner_id: user::Id",
                created_at  as "created_at: CreatedAt",
                updated_at  as "updated_at: UpdatedAt"
            "#,
            name,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(list)
    }

    /// Deletes a list, and with it everything on it.
    ///
    /// `items.list_id` and `list_members.list_id` are `ON DELETE CASCADE`, so a list
    /// with items on it is not [`Error::InUse`] — the items go too. That is the
    /// opposite of [`super::unit`], where a referenced row is held back, and the
    /// difference is deliberate: an item without its list is meaningless, whereas a
    /// unit outlives any one item.
    pub async fn delete(pool: &sqlx::SqlitePool, id: Id) -> Result<()> {
        let result = sqlx::query!(r#"DELETE FROM lists WHERE id = ?1"#, id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Fetches one page of the lists a user can see — owned, or shared with them.
    ///
    /// Lists are scoped to their owner, so there is no unscoped `list`: an endpoint
    /// that could page over everybody's lists is one refactor away from leaking
    /// them. The `lists_by_user` index covers the filter.
    ///
    /// `total` counts that owner's lists, not the table's, and is a second statement
    /// — see [`super::unit::Unit::list`] for why it is not folded into the page
    /// query.
    pub async fn visible_to(
        pool: &sqlx::SqlitePool,
        owner_id: user::Id,
        page: Paging,
        order_by: OrderBy<Field>,
    ) -> Result<OffsetPage<List>> {
        let field: &'static str = order_by.field.into();
        let direction: &'static str = order_by.direction.into();

        let limit = page.limit();
        let offset = page.offset();

        let lists = sqlx::query_as!(
            List,
            r#"
        SELECT
            id          as "id: Id",
            name        as "name: Name",
            owner_id    as "owner_id: user::Id",
            created_at  as "created_at: CreatedAt",
            updated_at  as "updated_at: UpdatedAt"
        FROM lists
        WHERE owner_id = ?1
           OR id IN (SELECT list_id FROM list_members WHERE user_id = ?1)
        ORDER BY
            CASE
                WHEN ?3 = 'ascending' THEN
                    CASE ?2
                        WHEN 'id' THEN id
                        WHEN 'name' THEN name
                        WHEN 'created_at' THEN created_at
                        WHEN 'updated_at' THEN updated_at
                    END
                END ASC NULLS LAST,
            CASE
                WHEN ?3 = 'descending' THEN
                    CASE ?2
                        WHEN 'id' THEN id
                        WHEN 'name' THEN name
                        WHEN 'created_at' THEN created_at
                        WHEN 'updated_at' THEN updated_at
                    END
            END DESC NULLS LAST,
            -- keeps paging deterministic when the sort key ties
            id ASC
        LIMIT ?4 OFFSET ?5
        "#,
            owner_id,
            field,
            direction,
            limit,
            offset,
        )
        .fetch_all(pool)
        .await?;

        let total = sqlx::query_scalar!(
            r#"
            SELECT count(*) as "total!: i64" FROM lists
            WHERE owner_id = ?1
               OR id IN (SELECT list_id FROM list_members WHERE user_id = ?1)
            "#,
            owner_id
        )
        .fetch_one(pool)
        .await?;

        Ok(page.page_of(lists, total))
    }

    /// Fetches one list. A miss is [`Error::NotFound`], not `Ok(None)`.
    ///
    /// This does not scope to a user, so a caller holding someone else's list id gets
    /// their list — checking that the list belongs to the requester is the caller's
    /// job, and `owner_id` is on the row so it can.
    pub async fn get(pool: &sqlx::SqlitePool, by: Lookup) -> Result<List> {
        let list = match by {
            Lookup::Id(v) => {
                sqlx::query_as!(
                    List,
                    r#"
                SELECT
                    id          as "id: Id",
                    name        as "name: Name",
                    owner_id    as "owner_id: user::Id",
                    created_at  as "created_at: CreatedAt",
                    updated_at  as "updated_at: UpdatedAt"
                FROM lists
                WHERE id = ?1 "#,
                    v
                )
                .fetch_one(pool)
                .await?
            }
        };

        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;
    use strum::VariantArray;

    use super::*;
    use crate::models::{Direction, pool};

    /// Lists in `fixtures/lists.sql`.
    const SEEDED: i64 = 18;
    /// The busiest owner in the fixture, and how many lists they own.
    const BUSIEST: &str = "Ana María López";
    const BUSIEST_LISTS: i64 = 7;
    /// A seeded user who owns nothing — 12 of the 20 do.
    const LISTLESS: &str = "Zoë";

    /// Lists are reached through their owner, so the tests need a user id. Users are
    /// looked up by `sub` in the model; here the fixture identifies them by name.
    async fn user_id(pool: &SqlitePool, name: &str) -> Result<user::Id> {
        Ok(sqlx::query_scalar!(
            r#"SELECT id as "id!: user::Id" FROM users WHERE name = ?1"#,
            name
        )
        .fetch_one(pool)
        .await?)
    }

    fn all_lists() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by(field: Field, direction: Direction) -> OrderBy<Field> {
        OrderBy { field, direction }
    }

    async fn any_list(pool: &SqlitePool) -> Result<List> {
        let owner = user_id(pool, BUSIEST).await?;
        let mut page = List::visible_to(
            pool,
            owner,
            all_lists(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;
        Ok(page.items.swap_remove(0))
    }

    async fn count(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM lists"#)
                .fetch_one(pool)
                .await?,
        )
    }

    fn ids(p: &OffsetPage<List>) -> Vec<Id> {
        p.items.iter().map(|l| l.id).collect()
    }

    fn names(p: &OffsetPage<List>) -> Vec<Name> {
        p.items.iter().map(|l| l.name.clone()).collect()
    }

    fn created_ats(p: &OffsetPage<List>) -> Vec<CreatedAt> {
        p.items.iter().map(|l| l.created_at).collect()
    }

    fn updated_ats(p: &OffsetPage<List>) -> Vec<UpdatedAt> {
        p.items.iter().map(|l| l.updated_at).collect()
    }

    // ---------------------------------------------------------------- create

    #[rstest]
    #[case::plain("Weeknight dinners", Ok("Weeknight dinners"))]
    #[case::trims_whitespace("   Weeknight dinners  ", Ok("Weeknight dinners"))]
    // a list name is read back by a person, so its case is left alone
    #[case::keeps_case("Fruit & VEG", Ok("Fruit & VEG"))]
    #[case::rejects_empty("", Err(Error::InvalidInput))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let got = List::create(&pool, owner, Name(input.into())).await;

        match (got, expected) {
            (Ok(list), Ok(want)) => {
                assert_eq!(list.name, Name(want.into()));
                assert_eq!(list.owner_id, owner, "created for the owner given");
                assert_eq!(
                    list.updated_at.0, list.created_at.0,
                    "a list starts out unedited"
                );
                assert_eq!(
                    List::get(&pool, Lookup::Id(list.id)).await?,
                    list,
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

    /// Two lists may share a name, even for one owner — nothing about a list is
    /// unique but its id. This is the opposite of [`super::unit`], and it is why
    /// [`Lookup`] has no `Name` variant.
    #[rstest]
    #[tokio::test]
    async fn create_allows_a_duplicate_name(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let first = List::create(&pool, owner, Name("Dairy".into())).await?;
        let second = List::create(&pool, owner, Name("Dairy".into())).await?;

        assert_ne!(first.id, second.id, "two lists, same name");
        assert_eq!(count(&pool).await?, 2);
        Ok(())
    }

    #[rstest]
    #[case::at_the_limit(MAX_NAME, Ok(()))]
    #[case::one_over_the_limit(MAX_NAME + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_the_name_length(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let got = List::create(&pool, owner, Name("x".repeat(length)))
            .await
            .map(|_| ());

        assert_eq!(got, expected, "a name of {length} characters");
        Ok(())
    }

    /// Starting a list for a user who does not exist is the caller's mistake, and
    /// must not be confused with the `InUse` kind of foreign-key failure.
    #[rstest]
    #[tokio::test]
    async fn create_rejects_an_unknown_owner(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = List::create(&pool, user::Id(9999), Name("orphan".into())).await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(count(&pool).await?, 0);
        Ok(())
    }

    // ---------------------------------------------------------------- update

    #[rstest]
    #[case::renames("Renamed", Ok("Renamed"))]
    #[case::trims_whitespace("  Renamed  ", Ok("Renamed"))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    #[tokio::test]
    async fn update(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let before = any_list(&pool).await?;

        let result = List::update(&pool, before.id, Name(input.into())).await;

        match (result, expected) {
            (Ok(after), Ok(want)) => {
                assert_eq!(after.id, before.id);
                assert_eq!(after.name, Name(want.into()));
                assert_eq!(
                    after.owner_id, before.owner_id,
                    "a rename is not a transfer"
                );
                assert_eq!(
                    after.created_at, before.created_at,
                    "renaming must not restamp created_at"
                );
                assert!(
                    after.updated_at.0 >= before.updated_at.0,
                    "updated_at moved backwards: {:?} -> {:?}",
                    before.updated_at,
                    after.updated_at
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                let unchanged = List::get(&pool, Lookup::Id(before.id)).await?;
                assert_eq!(
                    unchanged.name, before.name,
                    "a rejected rename must leave the row alone"
                );
                assert_eq!(
                    unchanged.updated_at, before.updated_at,
                    "a rejected rename must not stamp updated_at either"
                );
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    /// The fixture's `updated_at` values are all in the past, so a successful rename
    /// has to move the stamp forward for this to hold.
    #[rstest]
    #[tokio::test]
    async fn update_stamps_updated_at(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let before = any_list(&pool).await?;

        let after = List::update(&pool, before.id, Name("Renamed".into())).await?;

        assert!(
            after.updated_at.0 > before.updated_at.0,
            "expected a newer stamp than {:?}, got {:?}",
            before.updated_at,
            after.updated_at
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn update_reports_a_miss(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = List::update(&pool, Id(9999), Name("nothing".into())).await;

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
    #[tokio::test]
    async fn delete(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list = any_list(&pool).await?;

        List::delete(&pool, list.id).await?;

        assert!(
            matches!(
                List::get(&pool, Lookup::Id(list.id)).await,
                Err(Error::NotFound)
            ),
            "the row is gone"
        );
        assert_eq!(count(&pool).await?, SEEDED - 1);

        let result = List::delete(&pool, list.id).await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "deleting it twice reports the miss, got {result:?}"
        );
        Ok(())
    }

    /// A list with items on it deletes anyway, taking them with it — the opposite of
    /// [`super::unit`], where a referenced row is held back.
    #[rstest]
    #[tokio::test]
    async fn delete_takes_the_items_with_it(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list = sqlx::query_scalar!(
            r#"SELECT list_id as "id!: Id" FROM items GROUP BY list_id ORDER BY count(*) DESC LIMIT 1"#
        )
        .fetch_one(&pool)
        .await?;
        let items = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM items WHERE list_id = ?1"#,
            list
        )
        .fetch_one(&pool)
        .await?;
        assert!(items > 0, "need a list with items on it");

        List::delete(&pool, list).await?;

        let left = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM items WHERE list_id = ?1"#,
            list
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(left, 0, "the list's {items} items went with it");
        Ok(())
    }

    // ---------------------------------------------------------------- lookup

    #[rstest]
    #[tokio::test]
    async fn get(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let want = any_list(&pool).await?;

        assert_eq!(List::get(&pool, Lookup::Id(want.id)).await?, want);
        Ok(())
    }

    #[rstest]
    #[case::missing_id(Lookup::Id(Id(9999)))]
    #[case::zero_id(Lookup::Id(Id(0)))]
    #[tokio::test]
    async fn get_reports_a_miss(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] lookup: Lookup,
    ) {
        assert!(matches!(
            List::get(&pool, lookup).await,
            Err(Error::NotFound)
        ));
    }

    // --------------------------------------------------------------- scoping

    #[rstest]
    #[tokio::test]
    async fn visible_to_returns_only_that_users_lists(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let page = List::visible_to(
            &pool,
            owner,
            all_lists(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert_eq!(page.total, BUSIEST_LISTS, "counts only their lists");
        assert_eq!(page.items.len(), BUSIEST_LISTS as usize);
        assert!(
            page.items.iter().all(|l| l.owner_id == owner),
            "someone else's list leaked into the page"
        );
        assert!(
            count(&pool).await? > BUSIEST_LISTS,
            "other lists exist to leak"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn visible_to_is_empty_for_someone_who_owns_nothing(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let nobody = user_id(&pool, LISTLESS).await?;

        let page = List::visible_to(
            &pool,
            nobody,
            all_lists(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert!(page.items.is_empty());
        assert_eq!(page.total, 0, "the total is theirs, not the table's");
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_more);
        Ok(())
    }

    // -------------------------------------------------------------- ordering

    struct OrderCase {
        order_by: OrderBy<Field>,
        assert: fn(&OffsetPage<List>),
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
    #[case::updated_at_ascending(OrderCase {
        order_by: OrderBy { field: Field::UpdatedAt, direction: Direction::Ascending },
        assert: |p| assert!(updated_ats(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", updated_ats(p)),
    })]
    #[case::updated_at_descending(OrderCase {
        order_by: OrderBy { field: Field::UpdatedAt, direction: Direction::Descending },
        assert: |p| assert!(updated_ats(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", updated_ats(p)),
    })]
    #[tokio::test]
    async fn visible_to_orders_by_every_field(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: OrderCase,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let page = List::visible_to(&pool, owner, all_lists(), c.order_by).await?;
        assert_eq!(page.items.len(), BUSIEST_LISTS as usize);
        (c.assert)(&page);
        Ok(())
    }

    /// Each field must produce a *different* order. A [`Field`] variant with no
    /// matching arm in the SQL `CASE` falls through to NULL for every row, which
    /// orders nothing and raises no error — this is what catches that.
    ///
    /// It needs enough rows to go round: with four fields in two directions there are
    /// eight orders to keep apart, which is why the fixture gives this owner seven
    /// lists rather than three.
    #[rstest]
    #[tokio::test]
    async fn visible_to_every_field_changes_the_order(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let mut orders = Vec::new();
        for &field in Field::VARIANTS {
            for direction in [Direction::Ascending, Direction::Descending] {
                let page =
                    List::visible_to(&pool, owner, all_lists(), by(field, direction)).await?;
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
        lists: usize,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_page(
        PageCase { page: Paging { number: 1, size: 2 }, lists: 2, total_pages: 4, has_more: true }
    )]
    #[case::middle_page(
        PageCase { page: Paging { number: 2, size: 2 }, lists: 2, total_pages: 4, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { page: Paging { number: 4, size: 2 }, lists: 1, total_pages: 4, has_more: false }
    )]
    #[case::page_larger_than_their_lists(
        PageCase { page: Paging { number: 1, size: 100 }, lists: BUSIEST_LISTS as usize, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { page: Paging { number: 99, size: 2 }, lists: 0, total_pages: 4, has_more: false }
    )]
    // a negative LIMIT means "no limit" to SQLite; Paging::limit clamps it so that a
    // bad page size cannot dump every list the user owns
    #[case::negative_size_is_empty(
        PageCase { page: Paging { number: 1, size: -1 }, lists: 0, total_pages: 0, has_more: true }
    )]
    // offset would overflow i64 and panic in debug without the saturating multiply
    #[case::huge_page_number(
        PageCase { page: Paging { number: i64::MAX, size: 2 }, lists: 0, total_pages: 4, has_more: false }
    )]
    #[tokio::test]
    async fn visible_to_pages(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: PageCase,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;

        let page =
            List::visible_to(&pool, owner, c.page, by(Field::Id, Direction::Ascending)).await?;

        assert_eq!(page.items.len(), c.lists, "lists on the page");
        assert_eq!(
            page.total, BUSIEST_LISTS,
            "total is independent of the page"
        );
        assert_eq!(page.total_pages, c.total_pages);
        assert_eq!(page.has_more, c.has_more);
        Ok(())
    }

    #[rstest]
    #[case::by_id(Field::Id)]
    #[case::by_name(Field::Name)]
    #[case::by_created_at(Field::CreatedAt)]
    #[case::by_updated_at(Field::UpdatedAt)]
    #[tokio::test]
    async fn visible_to_walks_every_list_exactly_once(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] field: Field,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;
        let mut seen = Vec::new();
        let mut number = 1;

        loop {
            let page = List::visible_to(
                &pool,
                owner,
                Paging { number, size: 2 },
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
        assert_eq!(seen.len(), BUSIEST_LISTS as usize, "paged over {seen:?}");
        assert_eq!(unique.len(), BUSIEST_LISTS as usize, "repeated a list");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn visible_to_totals_ignore_other_owners(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let owner = user_id(&pool, BUSIEST).await?;
        let other = user_id(&pool, LISTLESS).await?;
        let order = by(Field::Id, Direction::Ascending);

        let before = List::visible_to(&pool, owner, all_lists(), order).await?;
        assert_eq!(before.total, BUSIEST_LISTS);

        List::create(&pool, other, Name("not theirs".into())).await?;
        let after = List::visible_to(&pool, owner, all_lists(), order).await?;
        assert_eq!(
            after.total, BUSIEST_LISTS,
            "someone else's list changed nothing"
        );

        List::create(&pool, owner, Name("theirs".into())).await?;
        let mine = List::visible_to(&pool, owner, all_lists(), order).await?;
        assert_eq!(mine.total, BUSIEST_LISTS + 1);
        Ok(())
    }
}
