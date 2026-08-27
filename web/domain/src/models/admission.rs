//! Who may sign in at all, as rows.
//!
//! The question before authorisation: whether this person gets an [`Actor`], not what
//! they may do with a list once they have one. A personal service is not made private
//! by owning the domain — anybody with a Google or Apple account can complete a
//! sign-in flow, and without this every one of them becomes a user on first sight.
//!
//! Nothing here decides anything. It reads and writes; [`crate::service::admission`]
//! is where the rules are.
//!
//! [`Actor`]: crate::service::Actor

use time::OffsetDateTime;

use super::user::{self, Email};
use super::{Error, Result};

// Scaffold the note and the timestamp
string!(Note);
timestamp!(AddedAt);

/// One admitted address, as the owner's screen shows it.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Admitted {
    pub email: Email,
    /// Who it turned out to be, once they signed in. `None` means nobody has used
    /// this address yet, which is the difference between "invited" and "here".
    pub user_id: Option<user::Id>,
    /// `None` where the row was seeded from configuration rather than added by a
    /// person.
    pub added_by: Option<user::Id>,
    pub added_at: AddedAt,
    pub note: Option<Note>,
}

/// The address as it is stored and compared: trimmed, then lowercased.
///
/// In Rust rather than SQL for the same reason [`super::history::key`] gives —
/// SQLite's `lower()` folds ASCII only.
pub fn key(email: &Email) -> String {
    email.0.trim().to_lowercase()
}

impl Admitted {
    /// Every admitted address, oldest first.
    pub async fn all(pool: &sqlx::SqlitePool) -> Result<Vec<Self>> {
        Ok(sqlx::query_as!(
            Self,
            r#"
            SELECT email as "email: Email",
                   user_id as "user_id: user::Id",
                   added_by as "added_by: user::Id",
                   added_at as "added_at: AddedAt",
                   note as "note: Note"
              FROM admitted_emails
             ORDER BY added_at, email
            "#
        )
        .fetch_all(pool)
        .await?)
    }

    /// Admits an address. Admitting one twice is not an error — it is a double-click.
    ///
    /// Deliberately does not clear `user_id`: re-admitting somebody who is already
    /// here should not forget who they are.
    pub async fn add(
        pool: &sqlx::SqlitePool,
        email: &Email,
        added_by: user::Id,
        note: Option<&Note>,
    ) -> Result<()> {
        let email = key(email);
        let note = note.map(|n| n.0.as_str());

        sqlx::query!(
            r#"
            INSERT INTO admitted_emails (email, added_by, note)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(email) DO UPDATE SET note = coalesce(excluded.note, note)
            "#,
            email,
            added_by,
            note,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Admits an address that nobody added, or that the earliest user is credited
    /// with. The configuration path — see `service::admission::seed`.
    pub async fn seed(
        pool: &sqlx::SqlitePool,
        email: &Email,
        added_by: Option<user::Id>,
    ) -> Result<()> {
        let email = key(email);

        sqlx::query!(
            r#"
            INSERT INTO admitted_emails (email, added_by) VALUES (?1, ?2)
            ON CONFLICT(email) DO NOTHING
            "#,
            email,
            added_by,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Withdraws an address, and says whether there was one.
    pub async fn remove(pool: &sqlx::SqlitePool, email: &Email) -> Result<bool> {
        let email = key(email);

        let result = sqlx::query!(r#"DELETE FROM admitted_emails WHERE email = ?1"#, email)
            .execute(pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Whether this address is admitted, ignoring whether anybody has used it.
    ///
    /// The check for somebody arriving for the first time. Everyone who has been here
    /// before is answered by [`Self::admits_user`] instead.
    pub async fn admits_email(pool: &sqlx::SqlitePool, email: &Email) -> Result<bool> {
        let email = key(email);

        Ok(sqlx::query_scalar!(
            r#"SELECT count(*) FROM admitted_emails WHERE email = ?1"#,
            email
        )
        .fetch_one(pool)
        .await?
            > 0)
    }

    /// Whether this person is admitted, whatever address they arrive with now.
    pub async fn admits_user(pool: &sqlx::SqlitePool, user_id: user::Id) -> Result<bool> {
        Ok(sqlx::query_scalar!(
            r#"SELECT count(*) FROM admitted_emails WHERE user_id = ?1"#,
            user_id
        )
        .fetch_one(pool)
        .await?
            > 0)
    }

    /// Ties an admitted address to the person who turned out to be behind it.
    ///
    /// Called on a successful sign-in. Idempotent, and silent when the address is not
    /// admitted — which is the open-server case, where there is no row to bind.
    pub async fn bind(pool: &sqlx::SqlitePool, email: &Email, user_id: user::Id) -> Result<()> {
        let email = key(email);

        sqlx::query!(
            r#"UPDATE admitted_emails SET user_id = ?2 WHERE email = ?1 AND user_id IS NULL"#,
            email,
            user_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}

/// What is true of this server rather than of anybody on it.
pub struct Server;

impl Server {
    /// Whether this server admits anybody the provider vouches for.
    pub async fn admits_anyone(pool: &sqlx::SqlitePool) -> Result<bool> {
        Ok(
            sqlx::query_scalar!(r#"SELECT admits_anyone FROM server WHERE id = 1"#)
                .fetch_one(pool)
                .await?
                != 0,
        )
    }

    pub async fn set_admits_anyone(pool: &sqlx::SqlitePool, open: bool) -> Result<()> {
        sqlx::query!(r#"UPDATE server SET admits_anyone = ?1 WHERE id = 1"#, open)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Whether anybody has claimed this server yet.
    pub async fn is_claimed(pool: &sqlx::SqlitePool) -> Result<bool> {
        Ok(
            sqlx::query_scalar!(r#"SELECT claimed_at FROM server WHERE id = 1"#)
                .fetch_one(pool)
                .await?
                .is_some(),
        )
    }

    /// Records that the server has been claimed, and says whether this call was the
    /// one that did it.
    ///
    /// Conditional on it being unclaimed, in one statement rather than a read and a
    /// write: two people opening the app in a home server's first minute is exactly
    /// how this race is lost, and SQLite's single writer makes losing it cheap only
    /// if the condition is in the statement.
    pub async fn claim(pool: &sqlx::SqlitePool) -> Result<bool> {
        let result = sqlx::query!(
            r#"UPDATE server SET claimed_at = unixepoch() WHERE id = 1 AND claimed_at IS NULL"#
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

impl Server {
    /// Claims the server *and* makes this person its owner, or does neither.
    ///
    /// One transaction, because the two halves are the same fact. A server marked
    /// claimed with nobody owning it is the state with no way back that does not
    /// involve `sqlite3` on the host, and it is exactly what a failure between two
    /// statements would produce.
    ///
    /// `false` means somebody else got there first.
    pub async fn claim_for(pool: &sqlx::SqlitePool, user_id: user::Id) -> Result<bool> {
        let mut tx = pool.begin().await?;

        let claimed = sqlx::query!(
            r#"UPDATE server SET claimed_at = unixepoch() WHERE id = 1 AND claimed_at IS NULL"#
        )
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        if !claimed {
            return Ok(false);
        }

        sqlx::query!(r#"UPDATE users SET is_owner = 1 WHERE id = ?1"#, user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(true)
    }
}

/// Everybody who may administer this server.
pub async fn owners(pool: &sqlx::SqlitePool) -> Result<Vec<user::Id>> {
    Ok(
        sqlx::query_scalar!(r#"SELECT id as "id: user::Id" FROM users WHERE is_owner <> 0"#)
            .fetch_all(pool)
            .await?,
    )
}

/// How many people may administer this server.
///
/// Its own query rather than `owners().len()`, because the only caller counts and the
/// rule it enforces — never zero — is the one thing that must not be got wrong.
pub async fn owner_count(pool: &sqlx::SqlitePool) -> Result<i64> {
    Ok(
        sqlx::query_scalar!(r#"SELECT count(*) FROM users WHERE is_owner <> 0"#)
            .fetch_one(pool)
            .await?,
    )
}

/// Promotes or demotes. The rules about *when* are in the service layer.
pub async fn set_owner(pool: &sqlx::SqlitePool, user_id: user::Id, owner: bool) -> Result<()> {
    let result = sqlx::query!(
        r#"UPDATE users SET is_owner = ?2 WHERE id = ?1"#,
        user_id,
        owner
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(Error::NotFound);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pool;
    use rstest::rstest;
    use sqlx::SqlitePool;

    fn email(address: &str) -> Email {
        Email(address.to_string())
    }

    #[test]
    fn an_address_is_keyed_however_it_is_typed() {
        assert_eq!(key(&email("  Me@Example.COM ")), "me@example.com");
    }

    #[rstest]
    #[tokio::test]
    async fn an_admitted_address_is_admitted(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        Admitted::add(&pool, &email("her@example.com"), user::Id(1), None)
            .await
            .unwrap();

        assert!(Admitted::admits_email(&pool, &email("her@example.com")).await.unwrap());
        // However it arrives, or a capitalised sign-in locks out the person who was
        // admitted in lower case.
        assert!(Admitted::admits_email(&pool, &email(" HER@Example.com ")).await.unwrap());
        assert!(!Admitted::admits_email(&pool, &email("stranger@example.com")).await.unwrap());
    }

    /// The reason `user_id` exists: a provider address is not stable, and somebody who
    /// changes theirs must not be locked out of a server holding their own lists.
    #[rstest]
    #[tokio::test]
    async fn once_bound_a_person_is_admitted_under_any_address(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        Admitted::add(&pool, &email("old@example.com"), user::Id(1), None).await.unwrap();
        Admitted::bind(&pool, &email("old@example.com"), user::Id(2)).await.unwrap();

        assert!(Admitted::admits_user(&pool, user::Id(2)).await.unwrap());
        assert!(!Admitted::admits_user(&pool, user::Id(3)).await.unwrap());
    }

    /// Binding twice must not move an address from one person to another — that would
    /// be a way to inherit somebody's admission by signing in after them.
    #[rstest]
    #[tokio::test]
    async fn an_address_binds_to_the_first_person_only(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        Admitted::add(&pool, &email("shared@example.com"), user::Id(1), None).await.unwrap();
        Admitted::bind(&pool, &email("shared@example.com"), user::Id(2)).await.unwrap();
        Admitted::bind(&pool, &email("shared@example.com"), user::Id(3)).await.unwrap();

        assert!(Admitted::admits_user(&pool, user::Id(2)).await.unwrap());
        assert!(!Admitted::admits_user(&pool, user::Id(3)).await.unwrap());
    }

    /// Admitting somebody who is already here must not forget who they are.
    #[rstest]
    #[tokio::test]
    async fn re_admitting_keeps_the_binding(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        Admitted::add(&pool, &email("her@example.com"), user::Id(1), None).await.unwrap();
        Admitted::bind(&pool, &email("her@example.com"), user::Id(2)).await.unwrap();
        Admitted::add(&pool, &email("her@example.com"), user::Id(1), Some(&Note("mum".into())))
            .await
            .unwrap();

        assert!(Admitted::admits_user(&pool, user::Id(2)).await.unwrap());
        let listed = Admitted::all(&pool).await.unwrap();
        assert_eq!(listed.len(), 1, "a second row was written");
        assert_eq!(listed[0].note, Some(Note("mum".into())));
    }

    #[rstest]
    #[tokio::test]
    async fn withdrawing_says_whether_there_was_anything_to_withdraw(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        Admitted::add(&pool, &email("her@example.com"), user::Id(1), None).await.unwrap();

        assert!(Admitted::remove(&pool, &email("HER@example.com")).await.unwrap());
        assert!(!Admitted::remove(&pool, &email("her@example.com")).await.unwrap());
        assert!(!Admitted::admits_email(&pool, &email("her@example.com")).await.unwrap());
    }

    /// Back to how a server arrives, since the fixture hands over one that is claimed
    /// and open so that tests about lists need not be tests about admission.
    async fn fresh(pool: &SqlitePool) {
        sqlx::raw_sql("UPDATE server SET admits_anyone = 0, claimed_at = NULL")
            .execute(pool)
            .await
            .unwrap();
    }

    /// The race a home server's first minute actually runs: two people opening the app
    /// at once, and exactly one of them claiming it.
    #[rstest]
    #[tokio::test]
    async fn a_server_is_claimed_once(#[future(awt)] pool: SqlitePool) {
        fresh(&pool).await;
        assert!(!Server::is_claimed(&pool).await.unwrap());

        assert!(Server::claim(&pool).await.unwrap(), "the first claim was refused");
        assert!(!Server::claim(&pool).await.unwrap(), "the server was claimed twice");
        assert!(Server::is_claimed(&pool).await.unwrap());
    }

    #[rstest]
    #[tokio::test]
    async fn an_open_server_says_so(#[future(awt)] pool: SqlitePool) {
        fresh(&pool).await;
        assert!(!Server::admits_anyone(&pool).await.unwrap(), "a fresh server is closed");

        Server::set_admits_anyone(&pool, true).await.unwrap();
        assert!(Server::admits_anyone(&pool).await.unwrap());
    }

    #[rstest]
    #[tokio::test]
    async fn owners_are_counted_and_listed(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        assert_eq!(owner_count(&pool).await.unwrap(), 0);

        set_owner(&pool, user::Id(1), true).await.unwrap();
        set_owner(&pool, user::Id(2), true).await.unwrap();
        assert_eq!(owner_count(&pool).await.unwrap(), 2);
        assert_eq!(owners(&pool).await.unwrap(), vec![user::Id(1), user::Id(2)]);

        set_owner(&pool, user::Id(1), false).await.unwrap();
        assert_eq!(owners(&pool).await.unwrap(), vec![user::Id(2)]);
    }

    #[rstest]
    #[tokio::test]
    async fn promoting_somebody_who_is_not_there(
        #[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) {
        assert!(matches!(
            set_owner(&pool, user::Id(9999), true).await,
            Err(Error::NotFound)
        ));
    }
}
