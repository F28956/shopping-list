use time::OffsetDateTime;

use super::user;
use super::{Error, Result};
use super::{OffsetPage, OrderBy, Paging};

// Scaffold Id, Body and CreatedAt
i64!(Id);
string!(Body);
timestamp!(CreatedAt);

// A note body is free text a person reads back, so only the padding comes off
trimmed!(Body);

/// The longest body `notes.body` accepts, in characters — keep in step with the
/// `CHECK` in the notes migration. Anything longer is [`Error::InvalidInput`].
pub const MAX_BODY: usize = 4096;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Note {
    pub id: Id,
    pub user_id: user::Id,
    pub body: Body,
    pub created_at: CreatedAt,
}

/// How a caller asks for a single note. Only `id` identifies one: a body is free
/// text and repeats, and a `user_id` matches every note that user wrote.
#[derive(Debug, Clone)]
pub enum Lookup {
    Id(Id),
}

/// What `for_user` may order by. Deliberately a separate enum from [`Lookup`] — the
/// set of sortable columns and the set of unique keys are not the same set.
///
/// Every variant added here needs a matching `WHEN` arm in both `CASE` branches of
/// the query. A variant without one silently sorts by nothing, which is what
/// `for_user_every_field_changes_the_order` exists to catch.
/// The default is `CreatedAt`: newest first is what a list of notes is for.
///
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Id,
    Body,
    #[default]
    CreatedAt,
}

impl Note {
    /// Writes a note for a user.
    ///
    /// A `user_id` that matches nobody is [`Error::InvalidInput`] — the reference is
    /// the caller's mistake, not a server fault.
    pub async fn create(pool: &sqlx::SqlitePool, user_id: user::Id, body: Body) -> Result<Note> {
        let body = body.trimmed();

        let note = sqlx::query_as!(
            Note,
            r#"
            INSERT INTO notes (user_id, body)
            VALUES (?1, ?2)
            RETURNING
                id          as "id!: Id",
                user_id     as "user_id: user::Id",
                body        as "body: Body",
                created_at  as "created_at!: CreatedAt"
            "#,
            user_id,
            body,
        )
        .fetch_one(pool)
        .await?;

        Ok(note)
    }

    /// Rewrites a note's body. The note stays with the user who wrote it: `user_id`
    /// is not writable, so a note cannot be moved to another person.
    pub async fn update(pool: &sqlx::SqlitePool, id: Id, body: Body) -> Result<Note> {
        let body = body.trimmed();

        let note = sqlx::query_as!(
            Note,
            r#"
            UPDATE notes SET body = ?1 WHERE id = ?2
            RETURNING
                id          as "id: Id",
                user_id     as "user_id: user::Id",
                body        as "body: Body",
                created_at  as "created_at: CreatedAt"
            "#,
            body,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(note)
    }

    /// Deletes a note. Nothing references a note, so this is never [`Error::InUse`].
    pub async fn delete(pool: &sqlx::SqlitePool, id: Id) -> Result<()> {
        let result = sqlx::query!(r#"DELETE FROM notes WHERE id = ?1"#, id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Fetches one page of a user's notes.
    ///
    /// Notes are scoped to their author, so there is no unscoped `list`: every
    /// caller has a user in hand, and an endpoint that could page over everybody's
    /// notes is one refactor away from leaking them. The `notes_by_user` index
    /// covers the filter.
    ///
    /// `total` counts that user's notes, not the table's, and is a second statement
    /// — see [`super::unit::Unit::list`] for why it is not folded into the page
    /// query.
    pub async fn for_user(
        pool: &sqlx::SqlitePool,
        user_id: user::Id,
        page: Paging,
        order_by: OrderBy<Field>,
    ) -> Result<OffsetPage<Note>> {
        let field: &'static str = order_by.field.into();
        let direction: &'static str = order_by.direction.into();

        let limit = page.limit();
        let offset = page.offset();

        let notes = sqlx::query_as!(
            Note,
            r#"
        SELECT
            id          as "id: Id",
            user_id     as "user_id: user::Id",
            body        as "body: Body",
            created_at  as "created_at: CreatedAt"
        FROM notes
        WHERE user_id = ?1
        ORDER BY
            CASE
                WHEN ?3 = 'ascending' THEN
                    CASE ?2
                        WHEN 'id' THEN id
                        WHEN 'body' THEN body
                        WHEN 'created_at' THEN created_at
                    END
                END ASC NULLS LAST,
            CASE
                WHEN ?3 = 'descending' THEN
                    CASE ?2
                        WHEN 'id' THEN id
                        WHEN 'body' THEN body
                        WHEN 'created_at' THEN created_at
                    END
            END DESC NULLS LAST,
            -- keeps paging deterministic when the sort key ties
            id ASC
        LIMIT ?4 OFFSET ?5
        "#,
            user_id,
            field,
            direction,
            limit,
            offset,
        )
        .fetch_all(pool)
        .await?;

        let total = sqlx::query_scalar!(
            r#"SELECT count(*) as "total!: i64" FROM notes WHERE user_id = ?1"#,
            user_id
        )
        .fetch_one(pool)
        .await?;

        Ok(page.page_of(notes, total))
    }

    /// Fetches one note. A miss is [`Error::NotFound`], not `Ok(None)`.
    ///
    /// This does not scope to a user, so a caller holding someone else's note id gets
    /// their note — checking that the note belongs to the requester is the caller's
    /// job, and `user_id` is on the row so it can.
    pub async fn get(pool: &sqlx::SqlitePool, by: Lookup) -> Result<Note> {
        let note = match by {
            Lookup::Id(v) => {
                sqlx::query_as!(
                    Note,
                    r#"
                SELECT
                    id          as "id: Id",
                    user_id     as "user_id: user::Id",
                    body        as "body: Body",
                    created_at  as "created_at: CreatedAt"
                FROM notes
                WHERE id = ?1 "#,
                    v
                )
                .fetch_one(pool)
                .await?
            }
        };

        Ok(note)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;
    use strum::VariantArray;

    use super::*;
    use crate::models::{Direction, pool};

    /// Notes in `fixtures/notes.sql`, and how they are shared out.
    const SEEDED: i64 = 12;
    /// The busiest author in the fixture, and how many notes they wrote.
    const BUSIEST: &str = "Ana María López";
    const BUSIEST_NOTES: i64 = 5;
    /// A seeded user who wrote nothing — the common case, not an edge case.
    const SILENT: &str = "Zoë";

    /// Notes are reached through their author, so the tests need a user id. Users are
    /// looked up by `sub` in the model; here the fixture identifies them by name.
    async fn user_id(pool: &SqlitePool, name: &str) -> Result<user::Id> {
        Ok(sqlx::query_scalar!(
            r#"SELECT id as "id!: user::Id" FROM users WHERE name = ?1"#,
            name
        )
        .fetch_one(pool)
        .await?)
    }

    fn all_notes() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by(field: Field, direction: Direction) -> OrderBy<Field> {
        OrderBy { field, direction }
    }

    async fn any_note(pool: &SqlitePool) -> Result<Note> {
        let author = user_id(pool, BUSIEST).await?;
        let mut page = Note::for_user(
            pool,
            author,
            all_notes(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;
        Ok(page.items.swap_remove(0))
    }

    async fn count(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM notes"#)
                .fetch_one(pool)
                .await?,
        )
    }

    fn ids(p: &OffsetPage<Note>) -> Vec<Id> {
        p.items.iter().map(|n| n.id).collect()
    }

    fn bodies(p: &OffsetPage<Note>) -> Vec<Body> {
        p.items.iter().map(|n| n.body.clone()).collect()
    }

    fn created_ats(p: &OffsetPage<Note>) -> Vec<CreatedAt> {
        p.items.iter().map(|n| n.created_at).collect()
    }

    // ---------------------------------------------------------------- create

    #[rstest]
    #[case::plain("Bring the tote bags", Ok("Bring the tote bags"))]
    #[case::trims_whitespace("   Bring the tote bags\n\n", Ok("Bring the tote bags"))]
    // a body is read back by a person, so its case is left alone
    #[case::keeps_case("Ask ANA about the Recipe", Ok("Ask ANA about the Recipe"))]
    #[case::keeps_interior_newlines("two\nlines", Ok("two\nlines"))]
    #[case::rejects_empty("", Err(Error::InvalidInput))]
    #[case::rejects_whitespace_only("   \n  ", Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        let got = Note::create(&pool, author, Body(input.into())).await;

        match (got, expected) {
            (Ok(note), Ok(want)) => {
                assert_eq!(note.body, Body(want.into()));
                assert_eq!(note.user_id, author, "written for the author given");
                assert_eq!(
                    Note::get(&pool, Lookup::Id(note.id)).await?,
                    note,
                    "the returned row is the one that was written"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(count(&pool).await?, 0, "a rejected body must not insert");
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    #[rstest]
    #[case::at_the_limit(MAX_BODY, Ok(()))]
    #[case::one_over_the_limit(MAX_BODY + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_the_body_length(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        let got = Note::create(&pool, author, Body("x".repeat(length)))
            .await
            .map(|_| ());

        assert_eq!(got, expected, "a body of {length} characters");
        Ok(())
    }

    /// Writing a note for a user who does not exist is the caller's mistake. It
    /// arrives as a foreign-key violation, which must not be confused with the
    /// `ON DELETE RESTRICT` kind that [`super::unit`] reports as `InUse`.
    #[rstest]
    #[tokio::test]
    async fn create_rejects_an_unknown_author(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = Note::create(&pool, user::Id(9999), Body("orphan".into())).await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(count(&pool).await?, 0);
        Ok(())
    }

    // ---------------------------------------------------------------- update

    #[rstest]
    #[case::rewrites("Different now", Ok("Different now"))]
    #[case::trims_whitespace("  Different now  ", Ok("Different now"))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    #[tokio::test]
    async fn update(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let before = any_note(&pool).await?;

        let result = Note::update(&pool, before.id, Body(input.into())).await;

        match (result, expected) {
            (Ok(after), Ok(want)) => {
                assert_eq!(after.id, before.id);
                assert_eq!(after.body, Body(want.into()));
                assert_eq!(after.user_id, before.user_id, "a note cannot change hands");
                assert_eq!(
                    after.created_at, before.created_at,
                    "rewriting must not restamp created_at"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(
                    Note::get(&pool, Lookup::Id(before.id)).await?.body,
                    before.body,
                    "a rejected rewrite must leave the row alone"
                );
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn update_reports_a_miss(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = Note::update(&pool, Id(9999), Body("nothing".into())).await;

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
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let note = any_note(&pool).await?;

        Note::delete(&pool, note.id).await?;

        assert!(
            matches!(
                Note::get(&pool, Lookup::Id(note.id)).await,
                Err(Error::NotFound)
            ),
            "the row is gone"
        );
        assert_eq!(count(&pool).await?, SEEDED - 1);

        let result = Note::delete(&pool, note.id).await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "deleting it twice reports the miss, got {result:?}"
        );
        Ok(())
    }

    /// `notes.user_id` is `ON DELETE CASCADE`, so notes are never what blocks a user
    /// from being removed.
    #[rstest]
    #[tokio::test]
    async fn delete_follows_the_author(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        crate::models::user::User::delete(&pool, author).await?;

        let left = Note::for_user(
            &pool,
            author,
            all_notes(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;
        assert!(left.items.is_empty(), "their notes went with them");
        assert_eq!(count(&pool).await?, SEEDED - BUSIEST_NOTES);
        Ok(())
    }

    // ---------------------------------------------------------------- lookup

    #[rstest]
    #[tokio::test]
    async fn get(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let want = any_note(&pool).await?;

        assert_eq!(Note::get(&pool, Lookup::Id(want.id)).await?, want);
        Ok(())
    }

    #[rstest]
    #[case::missing_id(Lookup::Id(Id(9999)))]
    #[case::zero_id(Lookup::Id(Id(0)))]
    #[tokio::test]
    async fn get_reports_a_miss(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] lookup: Lookup,
    ) {
        assert!(matches!(
            Note::get(&pool, lookup).await,
            Err(Error::NotFound)
        ));
    }

    // --------------------------------------------------------------- scoping

    /// The whole point of `for_user`: one author's notes, never anybody else's.
    #[rstest]
    #[tokio::test]
    async fn for_user_returns_only_that_users_notes(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        let page = Note::for_user(
            &pool,
            author,
            all_notes(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert_eq!(page.total, BUSIEST_NOTES, "counts only their notes");
        assert_eq!(page.items.len(), BUSIEST_NOTES as usize);
        assert!(
            page.items.iter().all(|n| n.user_id == author),
            "a note from someone else leaked into the page"
        );
        assert!(
            count(&pool).await? > BUSIEST_NOTES,
            "other notes exist to leak"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn for_user_is_empty_for_someone_who_wrote_nothing(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let silent = user_id(&pool, SILENT).await?;

        let page = Note::for_user(
            &pool,
            silent,
            all_notes(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert!(page.items.is_empty());
        assert_eq!(page.total, 0, "the total is theirs, not the table's");
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_more);
        Ok(())
    }

    /// A user who does not exist reads as a user with nothing, rather than an error.
    #[rstest]
    #[tokio::test]
    async fn for_user_is_empty_for_an_unknown_user(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let page = Note::for_user(
            &pool,
            user::Id(9999),
            all_notes(),
            by(Field::Id, Direction::Ascending),
        )
        .await?;

        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
        Ok(())
    }

    // -------------------------------------------------------------- ordering

    struct OrderCase {
        order_by: OrderBy<Field>,
        assert: fn(&OffsetPage<Note>),
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
    #[case::body_ascending(OrderCase {
        order_by: OrderBy { field: Field::Body, direction: Direction::Ascending },
        assert: |p| assert!(bodies(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", bodies(p)),
    })]
    #[case::body_descending(OrderCase {
        order_by: OrderBy { field: Field::Body, direction: Direction::Descending },
        assert: |p| assert!(bodies(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", bodies(p)),
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
    async fn for_user_orders_by_every_field(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: OrderCase,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        let page = Note::for_user(&pool, author, all_notes(), c.order_by).await?;
        assert_eq!(page.items.len(), BUSIEST_NOTES as usize);
        (c.assert)(&page);
        Ok(())
    }

    /// Each field must produce a *different* order. A [`Field`] variant with no
    /// matching arm in the SQL `CASE` falls through to NULL for every row, which
    /// orders nothing and raises no error — this is what catches that.
    ///
    /// Iterating `Field::VARIANTS` rather than a hand-written list is what makes it
    /// cover variants added later.
    #[rstest]
    #[tokio::test]
    async fn for_user_every_field_changes_the_order(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        let mut orders = Vec::new();
        for &field in Field::VARIANTS {
            for direction in [Direction::Ascending, Direction::Descending] {
                let page = Note::for_user(&pool, author, all_notes(), by(field, direction)).await?;
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
        notes: usize,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_page(
        PageCase { page: Paging { number: 1, size: 2 }, notes: 2, total_pages: 3, has_more: true }
    )]
    #[case::middle_page(
        PageCase { page: Paging { number: 2, size: 2 }, notes: 2, total_pages: 3, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { page: Paging { number: 3, size: 2 }, notes: 1, total_pages: 3, has_more: false }
    )]
    #[case::page_larger_than_their_notes(
        PageCase { page: Paging { number: 1, size: 100 }, notes: BUSIEST_NOTES as usize, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { page: Paging { number: 99, size: 2 }, notes: 0, total_pages: 3, has_more: false }
    )]
    // a negative LIMIT means "no limit" to SQLite; Paging::limit clamps it so that a
    // bad page size cannot dump every note the user has
    #[case::negative_size_is_empty(
        PageCase { page: Paging { number: 1, size: -1 }, notes: 0, total_pages: 0, has_more: true }
    )]
    // offset would overflow i64 and panic in debug without the saturating multiply
    #[case::huge_page_number(
        PageCase { page: Paging { number: i64::MAX, size: 2 }, notes: 0, total_pages: 3, has_more: false }
    )]
    #[tokio::test]
    async fn for_user_pages(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: PageCase,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;

        let page =
            Note::for_user(&pool, author, c.page, by(Field::Id, Direction::Ascending)).await?;

        assert_eq!(page.items.len(), c.notes, "notes on the page");
        assert_eq!(
            page.total, BUSIEST_NOTES,
            "total is independent of the page"
        );
        assert_eq!(page.total_pages, c.total_pages);
        assert_eq!(page.has_more, c.has_more);
        Ok(())
    }

    #[rstest]
    #[case::by_id(Field::Id)]
    #[case::by_body(Field::Body)]
    #[case::by_created_at(Field::CreatedAt)]
    #[tokio::test]
    async fn for_user_walks_every_note_exactly_once(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] field: Field,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;
        let mut seen = Vec::new();
        let mut number = 1;

        loop {
            let page = Note::for_user(
                &pool,
                author,
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
        assert_eq!(seen.len(), BUSIEST_NOTES as usize, "paged over {seen:?}");
        assert_eq!(unique.len(), BUSIEST_NOTES as usize, "repeated a note");
        Ok(())
    }

    /// Another author's note must not move this author's totals.
    #[rstest]
    #[tokio::test]
    async fn for_user_totals_ignore_other_authors(
        #[with(seeds!("fixtures/users.sql", "fixtures/notes.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let author = user_id(&pool, BUSIEST).await?;
        let other = user_id(&pool, SILENT).await?;
        let order = by(Field::Id, Direction::Ascending);

        let before = Note::for_user(&pool, author, all_notes(), order).await?;
        assert_eq!(before.total, BUSIEST_NOTES);

        Note::create(&pool, other, Body("not theirs".into())).await?;
        let after = Note::for_user(&pool, author, all_notes(), order).await?;
        assert_eq!(
            after.total, BUSIEST_NOTES,
            "someone else's note changed nothing"
        );

        Note::create(&pool, author, Body("theirs".into())).await?;
        let mine = Note::for_user(&pool, author, all_notes(), order).await?;
        assert_eq!(mine.total, BUSIEST_NOTES + 1);
        Ok(())
    }
}
