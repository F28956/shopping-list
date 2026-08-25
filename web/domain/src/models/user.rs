use time::OffsetDateTime;

use super::{Error, Result};
use super::{OffsetPage, OrderBy, Paging};

// Scaffold Id, Sub, Email, Name and CreatedAt
i64!(Id);
string!(Sub);
string!(Email);
string!(Name);
timestamp!(CreatedAt);

// An address is one address whatever case it arrives in, so it dedupes across case
normalized!(Email);
// `sub` is the identity key: folding its case would merge two people. `name` is free
// text a person reads back, so only the padding comes off.
trimmed!(Sub, Name);

/// Longest values `users` accepts, in characters — keep in step with the `CHECK`s in
/// the init migration. Anything longer is [`Error::InvalidInput`].
pub const MAX_SUB: usize = 255;
/// The maximum an address may be, per RFC 5321's 320-character path limit.
pub const MAX_EMAIL: usize = 320;
/// Long enough for the longest names in the fixtures with room to spare.
pub const MAX_NAME: usize = 128;

/// A user row.
///
/// The `email?:`/`name?:` annotations on every query below are load-bearing, not
/// noise. Adding the `CHECK` to those nullable columns flips sqlx's inference to
/// NOT NULL, and a `#[sqlx(transparent)]` newtype then decodes a NULL as
/// `Some(Email(""))` rather than `None` — silently, with no error anywhere. The `?`
/// forces the nullable decode back on.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct User {
    pub id: Id,
    pub sub: Sub,
    pub email: Option<Email>,
    pub name: Option<Name>,
    pub created_at: CreatedAt,
}

/// How a caller asks for a single user. Every variant must be able to identify at
/// most one row, so only the unique columns appear here: `users.email` and
/// `users.name` are neither unique nor `NOT NULL`, and `get`ting by one of those
/// would quietly return whichever row sorted first.
#[derive(Debug, Clone)]
pub enum Lookup {
    Id(Id),
    Sub(Sub),
}

/// What `list` may order by. Deliberately a separate enum from [`Lookup`] — the set
/// of sortable columns and the set of unique keys are not the same set.
///
/// Every variant added here needs a matching `WHEN` arm in both `CASE` branches of
/// the `list` query. A variant without one silently sorts by nothing, which is what
/// `list_every_field_changes_the_order` exists to catch.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Id,
    Sub,
    Email,
    Name,
    CreatedAt,
}

impl User {
    /// Inserts a user.
    ///
    /// `sub` is the identity assigned by the provider and is what makes the row
    /// unique; a second user with the same one is [`Error::Conflict`]. `email` and
    /// `name` are optional because a provider may withhold either.
    pub async fn create(
        pool: &sqlx::SqlitePool,
        sub: Sub,
        name: Option<Name>,
        email: Option<Email>,
    ) -> Result<User> {
        let sub = sub.trimmed();
        let name = name.map(Name::trimmed);
        let email = email.map(Email::normalized);

        let user = sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (sub, email, name)
            VALUES (?1, ?2, ?3)
            RETURNING
                id          as "id: Id",
                sub         as "sub: Sub",
                email       as "email?: Email",
                name        as "name?: Name",
                created_at  as "created_at!: CreatedAt"
            "#,
            sub,
            email,
            name,
        )
        .fetch_one(pool)
        .await?;

        Ok(user)
    }

    /// Resolves the user behind a verified identity, creating them on first sight.
    ///
    /// This is what authentication calls on every request, so it has to be idempotent:
    /// [`User::create`] is not, and using it here means the second request from a
    /// returning person collides with `users.sub UNIQUE` and fails.
    ///
    /// One statement, so two requests arriving together cannot race into two rows —
    /// the loser of the insert takes the `DO UPDATE` branch and gets the same row
    /// back. `created_at` is left alone, so it keeps meaning "first seen".
    ///
    /// The provider's claims win where it sent them and lose where it did not:
    /// `coalesce` refreshes a name or address that has changed since last login, and
    /// keeps the stored one when this token carries neither. Clearing a profile field
    /// is [`User::update`]'s job — signing in must never wipe it.
    pub async fn find_or_create(
        pool: &sqlx::SqlitePool,
        sub: Sub,
        name: Option<Name>,
        email: Option<Email>,
    ) -> Result<User> {
        let sub = sub.trimmed();
        let name = name.map(Name::trimmed);
        let email = email.map(Email::normalized);

        let user = sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (sub, email, name)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(sub) DO UPDATE SET
                email = coalesce(?2, users.email),
                name  = coalesce(?3, users.name)
            RETURNING
                id          as "id!: Id",
                sub         as "sub: Sub",
                email       as "email?: Email",
                name        as "name?: Name",
                created_at  as "created_at!: CreatedAt"
            "#,
            sub,
            email,
            name,
        )
        .fetch_one(pool)
        .await?;

        Ok(user)
    }

    /// Replaces a user's profile.
    ///
    /// Both columns are written every time, so `None` clears rather than keeps —
    /// there is no partial update. `sub` is not writable: it is the identity the
    /// provider assigned, and changing it would make this a different person.
    pub async fn update(
        pool: &sqlx::SqlitePool,
        id: Id,
        name: Option<Name>,
        email: Option<Email>,
    ) -> Result<User> {
        let name = name.map(Name::trimmed);
        let email = email.map(Email::normalized);

        let user = sqlx::query_as!(
            User,
            r#"
            UPDATE users SET name = ?1, email = ?2 WHERE id = ?3
            RETURNING
                id          as "id: Id",
                sub         as "sub: Sub",
                email       as "email?: Email",
                name        as "name?: Name",
                created_at  as "created_at: CreatedAt"
            "#,
            name,
            email,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(user)
    }

    /// Deletes a user, and with them everything they own.
    ///
    /// `lists.owner_id`, `list_members.user_id` and `notes.user_id` are all
    /// `ON DELETE CASCADE`, so this is not blockable the way [`super::unit`] is —
    /// there is no `InUse` case, the rows go too.
    pub async fn delete(pool: &sqlx::SqlitePool, id: Id) -> Result<()> {
        let result = sqlx::query!(r#"DELETE FROM users WHERE id = ?1"#, id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Fetches one page of users. The total is a second `count(*)`, which SQLite
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
    ) -> Result<OffsetPage<User>> {
        let field: &'static str = order_by.field.into();
        let direction: &'static str = order_by.direction.into();

        let limit = page.limit();
        let offset = page.offset();

        let users = sqlx::query_as!(
            User,
            r#"
        SELECT
            id          as "id: Id",
            sub         as "sub: Sub",
            email       as "email?: Email",
            name        as "name?: Name",
            created_at  as "created_at: CreatedAt"
        FROM users
        ORDER BY
            CASE
                WHEN ?2 = 'ascending' THEN
                    CASE ?1
                        WHEN 'id' THEN id
                        WHEN 'sub' THEN sub
                        WHEN 'email' THEN email
                        WHEN 'name' THEN name
                        WHEN 'created_at' THEN created_at
                    END
                END ASC NULLS LAST,
            CASE
                WHEN ?2 = 'descending' THEN
                    CASE ?1
                        WHEN 'id' THEN id
                        WHEN 'sub' THEN sub
                        WHEN 'email' THEN email
                        WHEN 'name' THEN name
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

        let total = sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM users"#)
            .fetch_one(pool)
            .await?;

        Ok(page.page_of(users, total))
    }

    /// Fetches one user. A miss is [`Error::NotFound`], not `Ok(None)`.
    ///
    /// `sub` is matched exactly. Unlike `units.name` it is neither normalised nor
    /// `COLLATE NOCASE`: it is an opaque identifier from the provider, and folding it
    /// would merge two identities.
    pub async fn get(pool: &sqlx::SqlitePool, by: Lookup) -> Result<User> {
        let user = match by {
            Lookup::Id(v) => {
                sqlx::query_as!(
                    User,
                    r#"
                SELECT
                    id          as "id: Id",
                    sub         as "sub: Sub",
                    email       as "email?: Email",
                    name        as "name?: Name",
                    created_at  as "created_at: CreatedAt"
                FROM users
                WHERE id = ?1 "#,
                    v
                )
                .fetch_one(pool)
                .await?
            }
            Lookup::Sub(v) => {
                let sub = v.trimmed();
                sqlx::query_as!(
                    User,
                    r#"
                SELECT
                    id          as "id: Id",
                    sub         as "sub: Sub",
                    email       as "email?: Email",
                    name        as "name?: Name",
                    created_at  as "created_at: CreatedAt"
                FROM users
                WHERE sub = ?1 "#,
                    sub
                )
                .fetch_one(pool)
                .await?
            }
        };

        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;
    use strum::VariantArray;

    use super::*;
    use crate::models::{Direction, pool};

    /// Users in `fixtures/users.sql`.
    const SEEDED: i64 = 20;

    fn all_users() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by(field: Field, direction: Direction) -> OrderBy<Field> {
        OrderBy { field, direction }
    }

    /// A seeded user, without hard-coding which one it is.
    async fn any_user(pool: &SqlitePool) -> Result<User> {
        let mut page = User::list(pool, all_users(), by(Field::Id, Direction::Ascending)).await?;
        Ok(page.items.swap_remove(0))
    }

    async fn count(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM users"#)
                .fetch_one(pool)
                .await?,
        )
    }

    fn ids(p: &OffsetPage<User>) -> Vec<Id> {
        p.items.iter().map(|u| u.id).collect()
    }

    fn subs(p: &OffsetPage<User>) -> Vec<Sub> {
        p.items.iter().map(|u| u.sub.clone()).collect()
    }

    fn names(p: &OffsetPage<User>) -> Vec<Option<Name>> {
        p.items.iter().map(|u| u.name.clone()).collect()
    }

    fn emails(p: &OffsetPage<User>) -> Vec<Option<Email>> {
        p.items.iter().map(|u| u.email.clone()).collect()
    }

    fn created_ats(p: &OffsetPage<User>) -> Vec<CreatedAt> {
        p.items.iter().map(|u| u.created_at).collect()
    }

    /// Every `Some` sorted in `direction`, and every `None` after all of them.
    fn sorted_nulls_last<T: Ord>(vals: &[Option<T>], direction: Direction) -> bool {
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
    #[case::sub_only(Sub("user-sub".into()), None, None, Ok(("user-sub", None, None)))]
    #[case::email_and_name(
        Sub("user-sub".into()),
        Some(Name("Jan van der Berg".into())),
        Some(Email("j.vanderberg@example.nl".into())),
        Ok(("user-sub", Some("Jan van der Berg"), Some("j.vanderberg@example.nl"))),
    )]
    #[case::idp_style_sub(
        Sub("apple|002517.4c8e1d9a2f6b3e7c5a1d8f0b2e4c6a90.1802".into()),
        None,
        None,
        Ok(("apple|002517.4c8e1d9a2f6b3e7c5a1d8f0b2e4c6a90.1802", None, None)),
    )]
    // an address is one address whatever case it arrives in
    #[case::lowercases_the_email(
        Sub("user-sub".into()),
        None,
        Some(Email("  Jan.VanDerBerg@Example.NL ".into())),
        Ok(("user-sub", None, Some("jan.vanderberg@example.nl"))),
    )]
    // but a name is read back by a person, so its case is left alone
    #[case::trims_the_name_without_folding_it(
        Sub("  user-sub  ".into()),
        Some(Name("   Ana María López ".into())),
        None,
        Ok(("user-sub", Some("Ana María López"), None)),
    )]
    #[case::rejects_an_empty_sub(Sub("".into()), None, None, Err(Error::InvalidInput))]
    #[case::rejects_a_whitespace_only_sub(Sub("   ".into()), None, None, Err(Error::InvalidInput))]
    #[case::rejects_an_empty_name(
        Sub("user-sub".into()), Some(Name("  ".into())), None, Err(Error::InvalidInput)
    )]
    #[case::rejects_an_empty_email(
        Sub("user-sub".into()), None, Some(Email("".into())), Err(Error::InvalidInput)
    )]
    #[tokio::test]
    async fn create(
        #[future(awt)] pool: SqlitePool,
        #[case] sub: Sub,
        #[case] name: Option<Name>,
        #[case] email: Option<Email>,
        #[case] expected: Result<(&str, Option<&str>, Option<&str>)>,
    ) -> Result<()> {
        let got = User::create(&pool, sub, name, email).await;

        match (got, expected) {
            (Ok(user), Ok((sub, name, email))) => {
                assert_eq!(user.sub, Sub(sub.into()));
                assert_eq!(user.name, name.map(|n| Name(n.into())));
                assert_eq!(user.email, email.map(|e| Email(e.into())));
                assert_eq!(
                    User::get(&pool, Lookup::Id(user.id)).await?,
                    user,
                    "the returned row is the one that was written"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(count(&pool).await?, 0, "a rejected user must not insert");
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    #[rstest]
    #[case::sub_at_the_limit(MAX_SUB, MAX_NAME, MAX_EMAIL, Ok(()))]
    #[case::sub_over_the_limit(MAX_SUB + 1, MAX_NAME, MAX_EMAIL, Err(Error::InvalidInput))]
    #[case::name_over_the_limit(MAX_SUB, MAX_NAME + 1, MAX_EMAIL, Err(Error::InvalidInput))]
    #[case::email_over_the_limit(MAX_SUB, MAX_NAME, MAX_EMAIL + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_every_length(
        #[future(awt)] pool: SqlitePool,
        #[case] sub: usize,
        #[case] name: usize,
        #[case] email: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let got = User::create(
            &pool,
            Sub("s".repeat(sub)),
            Some(Name("n".repeat(name))),
            Some(Email("e".repeat(email))),
        )
        .await
        .map(|_| ());

        assert_eq!(got, expected, "sub {sub}, name {name}, email {email}");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn create_rejects_duplicate_sub(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let existing = any_user(&pool).await?;

        let err = User::create(&pool, existing.sub, None, None)
            .await
            .expect_err("duplicate sub must not insert");

        assert_eq!(err, Error::Conflict);
        assert_eq!(
            count(&pool).await?,
            SEEDED,
            "the failed insert must not add a row"
        );
        Ok(())
    }

    /// The mirror of [`super::unit`]'s duplicate rule. `sub` has no `COLLATE NOCASE`
    /// and is not normalised, so two subs differing only in case are two people —
    /// harmonising the schema with `units.name` would merge identities.
    #[rstest]
    #[tokio::test]
    async fn create_treats_sub_case_as_significant(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let first = User::create(&pool, Sub("github|Casey".into()), None, None).await?;
        let second = User::create(&pool, Sub("github|casey".into()), None, None).await?;

        assert_ne!(first.id, second.id, "two subs, two users");
        assert_eq!(count(&pool).await?, 2);
        Ok(())
    }

    // -------------------------------------------------------- find_or_create

    /// The property that authentication depends on: calling it twice for the same
    /// person yields one user, not a `Conflict`.
    #[rstest]
    #[tokio::test]
    async fn find_or_create_is_idempotent(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let sub = Sub("google-oauth2|110583927461028374651".into());

        let first = User::find_or_create(&pool, sub.clone(), None, None).await?;
        let again = User::find_or_create(&pool, sub, None, None).await?;

        assert_eq!(first.id, again.id, "the same person, not a second row");
        assert_eq!(
            again.created_at, first.created_at,
            "created_at means first seen"
        );
        assert_eq!(count(&pool).await?, 1);
        Ok(())
    }

    /// Ten simultaneous first-time logins must still produce one user. The insert and
    /// the update are one statement, so the losers of the race take the `DO UPDATE`
    /// branch rather than failing.
    #[rstest]
    #[tokio::test]
    async fn find_or_create_survives_a_race(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let sub = Sub("github|9001".into());

        let calls = (0..10).map(|_| User::find_or_create(&pool, sub.clone(), None, None));
        let users = futures::future::try_join_all(calls).await?;

        let first = users[0].id;
        assert!(
            users.iter().all(|u| u.id == first),
            "raced into more than one user"
        );
        assert_eq!(count(&pool).await?, 1);
        Ok(())
    }

    /// A profile that changed at the provider since last login follows the person in.
    #[rstest]
    #[tokio::test]
    async fn find_or_create_refreshes_a_changed_profile(
        #[future(awt)] pool: SqlitePool,
    ) -> Result<()> {
        let sub = Sub("auth0|abc".into());
        User::find_or_create(
            &pool,
            sub.clone(),
            Some(Name("Ana López".into())),
            Some(Email("ana@example.com".into())),
        )
        .await?;

        let after = User::find_or_create(
            &pool,
            sub,
            Some(Name("Ana María López".into())),
            Some(Email("  Ana.Lopez@Example.COM ".into())),
        )
        .await?;

        assert_eq!(after.name, Some(Name("Ana María López".into())));
        assert_eq!(
            after.email,
            Some(Email("ana.lopez@example.com".into())),
            "normalised on the way in, like any other write"
        );
        Ok(())
    }

    /// A token that carries no name or address must not wipe the stored profile —
    /// providers withhold claims, and signing in is not an edit.
    #[rstest]
    #[tokio::test]
    async fn find_or_create_keeps_what_the_token_omits(
        #[future(awt)] pool: SqlitePool,
    ) -> Result<()> {
        let sub = Sub("apple|001482".into());
        let before = User::find_or_create(
            &pool,
            sub.clone(),
            Some(Name("Sofía Ruiz".into())),
            Some(Email("sofia@example.es".into())),
        )
        .await?;

        let after = User::find_or_create(&pool, sub, None, None).await?;

        assert_eq!(
            after.name, before.name,
            "a withheld claim must not clear the name"
        );
        assert_eq!(after.email, before.email, "nor the address");
        Ok(())
    }

    /// `sub` is the identity, and its case is significant — the same rule
    /// [`User::create`] follows.
    #[rstest]
    #[tokio::test]
    async fn find_or_create_treats_sub_case_as_significant(
        #[future(awt)] pool: SqlitePool,
    ) -> Result<()> {
        let one = User::find_or_create(&pool, Sub("github|Casey".into()), None, None).await?;
        let two = User::find_or_create(&pool, Sub("github|casey".into()), None, None).await?;

        assert_ne!(one.id, two.id, "two subs, two people");
        assert_eq!(count(&pool).await?, 2);
        Ok(())
    }

    #[rstest]
    #[case::empty_sub(Sub("".into()), Err(Error::InvalidInput))]
    #[case::whitespace_only_sub(Sub("   ".into()), Err(Error::InvalidInput))]
    #[case::sub_over_the_limit(Sub("s".repeat(MAX_SUB + 1)), Err(Error::InvalidInput))]
    #[tokio::test]
    async fn find_or_create_validates_like_create(
        #[future(awt)] pool: SqlitePool,
        #[case] sub: Sub,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let got = User::find_or_create(&pool, sub, None, None)
            .await
            .map(|_| ());

        assert_eq!(got, expected);
        assert_eq!(count(&pool).await?, 0);
        Ok(())
    }

    /// It finds people who arrived by other routes, not only ones it created.
    #[rstest]
    #[tokio::test]
    async fn find_or_create_finds_a_seeded_user(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let existing = any_user(&pool).await?;

        let found = User::find_or_create(&pool, existing.sub.clone(), None, None).await?;

        assert_eq!(found, existing, "same row, unchanged");
        assert_eq!(count(&pool).await?, SEEDED, "no row added");
        Ok(())
    }

    // ---------------------------------------------------------------- update

    #[rstest]
    #[tokio::test]
    async fn update_replaces_the_profile(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let before = any_user(&pool).await?;

        let after = User::update(
            &pool,
            before.id,
            Some(Name(" Þórunn Jónsdóttir ".into())),
            Some(Email(" Thorunn@Example.IS ".into())),
        )
        .await?;

        assert_eq!(after.id, before.id);
        assert_eq!(after.sub, before.sub, "sub is not writable");
        assert_eq!(
            after.created_at, before.created_at,
            "created_at is not restamped"
        );
        assert_eq!(
            after.name,
            Some(Name("Þórunn Jónsdóttir".into())),
            "trimmed, case kept"
        );
        assert_eq!(
            after.email,
            Some(Email("thorunn@example.is".into())),
            "normalised"
        );
        Ok(())
    }

    /// `None` clears the column rather than leaving it — there is no partial update.
    #[rstest]
    #[tokio::test]
    async fn update_clears_with_none(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let before = any_user(&pool).await?;
        assert!(
            before.name.is_some() && before.email.is_some(),
            "need a user with both set to make this meaningful"
        );

        let after = User::update(&pool, before.id, None, None).await?;

        assert_eq!(after.name, None);
        assert_eq!(after.email, None);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn update_reports_a_miss(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = User::update(&pool, Id(9999), None, None).await;

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

    /// The interaction that keeps profile editing unwired: `find_or_create` runs on
    /// every authenticated request and coalesces the provider's claims over what is
    /// stored, so a name a person set for themselves does not survive their next
    /// request. Whether the provider or the person wins is a decision nobody has
    /// made; this test is here so that changing the answer breaks something loud.
    #[rstest]
    #[tokio::test]
    async fn a_login_overwrites_a_self_chosen_name(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let sub = Sub("google-oauth2|self-namer".into());
        let user = User::find_or_create(
            &pool,
            sub.clone(),
            Some(Name("Robert Smith".into())),
            Some(Email("robert@example.com".into())),
        )
        .await?;

        // they rename themselves
        let edited = User::update(
            &pool,
            user.id,
            Some(Name("Bob".into())),
            Some(Email("robert@example.com".into())),
        )
        .await?;
        assert_eq!(edited.name, Some(Name("Bob".into())));

        // ...and the next request arrives with the provider's claims again
        let after = User::find_or_create(
            &pool,
            sub,
            Some(Name("Robert Smith".into())),
            Some(Email("robert@example.com".into())),
        )
        .await?;

        assert_eq!(
            after.name,
            Some(Name("Robert Smith".into())),
            "the provider's name won, so self-chosen names do not stick"
        );
        Ok(())
    }

    // ---------------------------------------------------------------- delete

    #[rstest]
    #[tokio::test]
    async fn delete(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let user = any_user(&pool).await?;

        User::delete(&pool, user.id).await?;

        assert!(
            matches!(
                User::get(&pool, Lookup::Id(user.id)).await,
                Err(Error::NotFound)
            ),
            "the row is gone"
        );
        assert_eq!(count(&pool).await?, SEEDED - 1);

        let result = User::delete(&pool, user.id).await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "deleting them twice reports the miss, got {result:?}"
        );
        Ok(())
    }

    /// Unlike a unit, a user is never `InUse`: `lists.owner_id` is `ON DELETE
    /// CASCADE`, so their lists go with them rather than blocking the delete.
    #[rstest]
    #[tokio::test]
    async fn delete_takes_the_lists_with_it(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let owner = sqlx::query_scalar!(
            r#"SELECT owner_id as "id!: Id" FROM lists GROUP BY owner_id ORDER BY count(*) DESC LIMIT 1"#
        )
        .fetch_one(&pool)
        .await?;
        let owned = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM lists WHERE owner_id = ?1"#,
            owner
        )
        .fetch_one(&pool)
        .await?;
        assert!(owned > 0, "need an owner with lists");

        User::delete(&pool, owner).await?;

        let left = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM lists WHERE owner_id = ?1"#,
            owner
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(left, 0, "the owner's {owned} lists went with them");
        Ok(())
    }

    // ---------------------------------------------------------------- lookup

    #[rstest]
    #[tokio::test]
    async fn get(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let want = any_user(&pool).await?;

        assert_eq!(User::get(&pool, Lookup::Id(want.id)).await?, want);
        assert_eq!(User::get(&pool, Lookup::Sub(want.sub.clone())).await?, want);
        Ok(())
    }

    /// `fetch_one` means a miss is an error, not `Ok(None)`. Every caller has to
    /// handle that, so it is worth stating.
    #[rstest]
    #[case::missing_id(Lookup::Id(Id(9999)))]
    #[case::zero_id(Lookup::Id(Id(0)))]
    #[case::unknown_sub(Lookup::Sub(Sub("nobody".into())))]
    #[case::empty_sub(Lookup::Sub(Sub("".into())))]
    #[tokio::test]
    async fn get_reports_a_miss(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] lookup: Lookup,
    ) {
        assert!(matches!(
            User::get(&pool, lookup).await,
            Err(Error::NotFound)
        ));
    }

    /// Where [`super::unit`] folds case on lookup, this must not.
    #[rstest]
    #[tokio::test]
    async fn get_by_sub_is_case_sensitive(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let existing = any_user(&pool).await?;
        let shouted = Sub(existing.sub.0.to_uppercase());
        assert_ne!(shouted, existing.sub, "need a sub with letters in it");

        assert!(matches!(
            User::get(&pool, Lookup::Sub(shouted)).await,
            Err(Error::NotFound)
        ));
        Ok(())
    }

    // -------------------------------------------------------------- ordering

    struct OrderCase {
        order_by: OrderBy<Field>,
        assert: fn(&OffsetPage<User>),
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
    #[case::sub_ascending(OrderCase {
        order_by: OrderBy { field: Field::Sub, direction: Direction::Ascending },
        assert: |p| assert!(subs(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", subs(p)),
    })]
    #[case::sub_descending(OrderCase {
        order_by: OrderBy { field: Field::Sub, direction: Direction::Descending },
        assert: |p| assert!(subs(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", subs(p)),
    })]
    #[case::email_ascending_nulls_last(OrderCase {
        order_by: OrderBy { field: Field::Email, direction: Direction::Ascending },
        assert: |p| {
            assert!(sorted_nulls_last(&emails(p), Direction::Ascending), "{:?}", emails(p));
            assert_eq!(emails(p).last(), Some(&None));
        },
    })]
    #[case::email_descending_nulls_last(OrderCase {
        order_by: OrderBy { field: Field::Email, direction: Direction::Descending },
        assert: |p| {
            assert!(sorted_nulls_last(&emails(p), Direction::Descending), "{:?}", emails(p));
            // NULLS LAST applies to both branches, not just the ascending one
            assert_eq!(emails(p).last(), Some(&None));
        },
    })]
    #[case::name_ascending_nulls_last(OrderCase {
        order_by: OrderBy { field: Field::Name, direction: Direction::Ascending },
        assert: |p| {
            assert!(sorted_nulls_last(&names(p), Direction::Ascending), "{:?}", names(p));
            assert_eq!(names(p).last(), Some(&None));
        },
    })]
    #[case::name_descending_nulls_last(OrderCase {
        order_by: OrderBy { field: Field::Name, direction: Direction::Descending },
        assert: |p| {
            assert!(sorted_nulls_last(&names(p), Direction::Descending), "{:?}", names(p));
            assert_eq!(names(p).last(), Some(&None));
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
    async fn list_orders_by_every_field(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: OrderCase,
    ) -> Result<()> {
        let page = User::list(&pool, all_users(), c.order_by).await?;
        assert_eq!(page.items.len(), SEEDED as usize);
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
    async fn list_every_field_changes_the_order(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let mut orders = Vec::new();
        for &field in Field::VARIANTS {
            for direction in [Direction::Ascending, Direction::Descending] {
                let page = User::list(&pool, all_users(), by(field, direction)).await?;
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
        users: usize,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_page(
        PageCase { page: Paging { number: 1, size: 6 }, users: 6, total_pages: 4, has_more: true }
    )]
    #[case::middle_page(
        PageCase { page: Paging { number: 2, size: 6 }, users: 6, total_pages: 4, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { page: Paging { number: 4, size: 6 }, users: 2, total_pages: 4, has_more: false }
    )]
    #[case::page_larger_than_the_table(
        PageCase { page: Paging { number: 1, size: 100 }, users: SEEDED as usize, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { page: Paging { number: 99, size: 6 }, users: 0, total_pages: 4, has_more: false }
    )]
    // a negative LIMIT means "no limit" to SQLite; Paging::limit clamps it so that a
    // bad page size cannot dump the whole table
    #[case::negative_size_is_empty(
        PageCase { page: Paging { number: 1, size: -1 }, users: 0, total_pages: 0, has_more: true }
    )]
    // offset would overflow i64 and panic in debug without the saturating multiply
    #[case::huge_page_number(
        PageCase { page: Paging { number: i64::MAX, size: 6 }, users: 0, total_pages: 4, has_more: false }
    )]
    #[tokio::test]
    async fn list_pages(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: PageCase,
    ) -> Result<()> {
        let page = User::list(&pool, c.page, by(Field::Id, Direction::Ascending)).await?;

        assert_eq!(page.items.len(), c.users, "users on the page");
        assert_eq!(page.total, SEEDED, "total is independent of the page");
        assert_eq!(page.total_pages, c.total_pages);
        assert_eq!(page.has_more, c.has_more);
        Ok(())
    }

    /// Walking `has_more` must reach every user exactly once, including the rows that
    /// sort last because their column is NULL.
    #[rstest]
    #[case::by_id(Field::Id)]
    #[case::by_sub(Field::Sub)]
    #[case::by_email(Field::Email)]
    #[case::by_name(Field::Name)]
    #[case::by_created_at(Field::CreatedAt)]
    #[tokio::test]
    async fn list_walks_every_user_exactly_once(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
        #[case] field: Field,
    ) -> Result<()> {
        let mut seen = Vec::new();
        let mut number = 1;

        loop {
            let page = User::list(
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
        assert_eq!(unique.len(), SEEDED as usize, "repeated a user");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_totals_track_inserts(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        // a size whose last page is partial, so one insert fills it rather than
        // spilling onto a new one
        const SIZE: i64 = 7;
        let pages = (SEEDED + SIZE - 1) / SIZE;
        let on_last = SEEDED - (pages - 1) * SIZE;
        assert!(on_last < SIZE, "SEEDED must not divide evenly into SIZE");

        let page = |n| Paging {
            number: n,
            size: SIZE,
        };
        let order = by(Field::Id, Direction::Ascending);

        let before = User::list(&pool, page(pages), order).await?;
        assert_eq!(before.total, SEEDED);
        assert_eq!(before.total_pages, pages);
        assert_eq!(before.items.len(), on_last as usize);
        assert!(!before.has_more);

        User::create(&pool, Sub("user-sub".into()), None, None).await?;

        let after = User::list(&pool, page(pages), order).await?;
        assert_eq!(after.total, SEEDED + 1);
        assert_eq!(after.total_pages, pages, "the new user fits the last page");
        assert_eq!(after.items.len(), on_last as usize + 1);
        assert!(!after.has_more);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_on_an_empty_table(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let page = User::list(
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
