//! Credentials this application issues.
//!
//! A provider's token is a claim about who somebody is; a session is this server's own
//! decision to keep believing it. The difference matters because Apple's identity token
//! lasts about ten minutes and cannot be refreshed silently — so with Apple the
//! provider's token is a bootstrap, exchanged once for one of these.
//!
//! Only the hash is stored, exactly as for [`super::invite`]: the token itself lives in
//! the client's keychain and in an `Authorization` header, so a leaked backup hands out
//! nothing.

use time::OffsetDateTime;

use super::user;
use super::{Error, Result};

string!(Token);
timestamp!(CreatedAt, LastUsedAt);

/// How long a session stands without being used.
///
/// Measured from the last request rather than from sign-in, so a phone in daily use
/// never signs itself out and one left in a drawer over a summer does. Three months is
/// the drawer.
pub const IDLE_DAYS: i64 = 90;

/// What a token hashes to, as lowercase hex.
///
/// SHA-256 with no salt, for the reason given in [`super::invite::hash`]: the token is
/// 256 bits of randomness, so there is no dictionary to defend against, and an unsalted
/// hash is what lets a lookup be a primary-key hit rather than a scan.
pub fn hash(token: &Token) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(token.0.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct Session;

impl Session {
    /// Records a session and returns nothing: the caller already holds the token, and
    /// this module is deliberately unable to hand it back later.
    pub async fn create(
        pool: &sqlx::SqlitePool,
        token: &Token,
        user_id: user::Id,
        provider: &str,
    ) -> Result<()> {
        let token_hash = hash(token);

        sqlx::query!(
            r#"
            INSERT INTO app_sessions (token_hash, user_id, provider)
            VALUES (?1, ?2, ?3)
            "#,
            token_hash,
            user_id,
            provider,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Who this token belongs to, moving its idle clock forward.
    ///
    /// The touch is the same statement as the lookup, so a session cannot be read
    /// without being kept alive — two statements would let a request that arrived at
    /// the wrong moment renew a session it then found expired.
    ///
    /// `NotFound` covers unknown, revoked and long-idle alike. Which of the three it
    /// was is the sender's business only in the sense that they should sign in again.
    pub async fn claim(pool: &sqlx::SqlitePool, token: &Token) -> Result<user::Id> {
        let token_hash = hash(token);
        let cutoff = OffsetDateTime::now_utc().unix_timestamp() - IDLE_DAYS * 86_400;

        sqlx::query_scalar!(
            r#"
            UPDATE app_sessions
               SET last_used_at = unixepoch()
             WHERE token_hash = ?1 AND last_used_at > ?2
            RETURNING user_id as "user_id: user::Id"
            "#,
            token_hash,
            cutoff,
        )
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)
    }

    /// Ends one session. Signing out on the phone leaves the Mac signed in.
    ///
    /// Silent about whether there was anything to end: a client that has lost track of
    /// its own token should still be able to say "forget this", and telling it whether
    /// the token was real is a way of asking.
    pub async fn revoke(pool: &sqlx::SqlitePool, token: &Token) -> Result<()> {
        let token_hash = hash(token);

        sqlx::query!(r#"DELETE FROM app_sessions WHERE token_hash = ?1"#, token_hash)
            .execute(pool)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pool;
    use rstest::rstest;
    use sqlx::SqlitePool;

    #[test]
    fn a_token_hashes_to_sixty_four_hex_characters() {
        let h = hash(&Token("hello".into()));
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[rstest]
    #[tokio::test]
    async fn a_session_resolves_to_the_person_it_was_made_for(#[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool) {
        let token = Token("a-token".into());
        Session::create(&pool, &token, user::Id(1), "apple").await.unwrap();

        assert_eq!(Session::claim(&pool, &token).await.unwrap(), user::Id(1));
    }

    #[rstest]
    #[tokio::test]
    async fn a_token_nobody_issued_resolves_to_nobody(#[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool) {
        let claimed = Session::claim(&pool, &Token("invented".into())).await;

        assert!(matches!(claimed, Err(Error::NotFound)));
    }

    /// Signing out has to actually end it, or "sign out" is a button that clears a
    /// screen.
    #[rstest]
    #[tokio::test]
    async fn a_revoked_session_stops_working(#[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool) {
        let token = Token("a-token".into());
        Session::create(&pool, &token, user::Id(1), "apple").await.unwrap();
        Session::revoke(&pool, &token).await.unwrap();

        assert!(matches!(Session::claim(&pool, &token).await, Err(Error::NotFound)));
    }

    #[rstest]
    #[tokio::test]
    async fn a_session_idle_past_the_limit_is_gone(#[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool) {
        let token = Token("a-token".into());
        Session::create(&pool, &token, user::Id(1), "apple").await.unwrap();

        let long_ago = OffsetDateTime::now_utc().unix_timestamp() - (IDLE_DAYS + 1) * 86_400;
        let token_hash = hash(&token);
        sqlx::query!(
            r#"UPDATE app_sessions SET last_used_at = ?2 WHERE token_hash = ?1"#,
            token_hash,
            long_ago
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(matches!(Session::claim(&pool, &token).await, Err(Error::NotFound)));
    }

    /// The idle clock is measured from the last request, so a session in use never
    /// reaches the limit.
    #[rstest]
    #[tokio::test]
    async fn using_a_session_puts_off_its_expiry(#[with(seeds!("fixtures/users.sql"))]
        #[future(awt)]
        pool: SqlitePool) {
        let token = Token("a-token".into());
        Session::create(&pool, &token, user::Id(1), "apple").await.unwrap();

        let nearly = OffsetDateTime::now_utc().unix_timestamp() - (IDLE_DAYS * 86_400 - 60);
        let token_hash = hash(&token);
        sqlx::query!(
            r#"UPDATE app_sessions SET last_used_at = ?2 WHERE token_hash = ?1"#,
            token_hash,
            nearly
        )
        .execute(&pool)
        .await
        .unwrap();

        Session::claim(&pool, &token).await.unwrap();

        let touched: i64 =
            sqlx::query_scalar!(r#"SELECT last_used_at FROM app_sessions WHERE token_hash = ?1"#, token_hash)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert!(touched > nearly, "the idle clock was not moved forward");
    }
}
