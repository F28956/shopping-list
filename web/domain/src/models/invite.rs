//! Invitations to share a list.
//!
//! An invitation is a bearer credential: whoever holds the link gets the role in it.
//! Only its hash is stored, so a leaked backup hands out nothing — the raw token
//! exists in the URL and nowhere else, exactly as a password would.

use time::OffsetDateTime;

use super::list::{self, Role};
use super::user;
use super::{Error, Result};

// Scaffold Token and the timestamps
string!(Token);
timestamp!(CreatedAt);
timestamp!(ExpiresAt);

/// How long an unused invitation stands.
///
/// A week: long enough to send and be read, short enough that a link forgotten in a
/// message thread stops working before it is forgotten about.
pub const VALID_FOR_DAYS: i64 = 7;

/// A stored invitation. The token itself is not here — only what it hashes to.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Invite {
    pub list_id: list::Id,
    pub role: Role,
    pub created_by: user::Id,
    pub created_at: CreatedAt,
    pub expires_at: ExpiresAt,
}

/// What a token hashes to, as lowercase hex.
///
/// SHA-256 with no salt on purpose: the token is 256 bits of randomness, so there is
/// no dictionary to defend against, and an unsalted hash is what lets a lookup be a
/// primary-key hit rather than a scan.
pub fn hash(token: &Token) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(token.0.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

impl Invite {
    /// Stores an invitation and returns nothing: the caller already holds the token,
    /// and this module is deliberately unable to hand it back later.
    pub async fn create(
        pool: &sqlx::SqlitePool,
        token: &Token,
        list_id: list::Id,
        role: Role,
        created_by: user::Id,
    ) -> Result<()> {
        let token_hash = hash(token);
        let expires_at = OffsetDateTime::now_utc().unix_timestamp() + VALID_FOR_DAYS * 86_400;

        sqlx::query!(
            r#"
            INSERT INTO list_invites (token_hash, list_id, role, created_by, expires_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            token_hash,
            list_id,
            role,
            created_by,
            expires_at,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Looks up a live invitation by its token.
    ///
    /// Expiry is enforced here rather than by a sweeper, so a stale row is never
    /// honoured even while it is still on disk. A miss and an expired invitation are
    /// the same answer: a link that does not work does not say why.
    pub async fn claim(pool: &sqlx::SqlitePool, token: &Token) -> Result<Invite> {
        let token_hash = hash(token);
        let now = OffsetDateTime::now_utc().unix_timestamp();

        sqlx::query_as!(
            Invite,
            r#"
            SELECT
                list_id    as "list_id: list::Id",
                role       as "role: Role",
                created_by as "created_by: user::Id",
                created_at as "created_at: CreatedAt",
                expires_at as "expires_at: ExpiresAt"
            FROM list_invites
            WHERE token_hash = ?1 AND expires_at > ?2
            "#,
            token_hash,
            now
        )
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)
    }

    /// Marks an invitation as used.
    ///
    /// Not deleted: a used invitation is a record of how somebody got access, which
    /// is worth keeping until the row is swept.
    pub async fn mark_used(pool: &sqlx::SqlitePool, token: &Token) -> Result<()> {
        let token_hash = hash(token);

        sqlx::query!(
            r#"UPDATE list_invites SET used_at = unixepoch() WHERE token_hash = ?1"#,
            token_hash
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Withdraws every outstanding invitation to a list.
    ///
    /// The only revocation offered, because it is the only one an owner can act on:
    /// they cannot tell one unused link from another, having never seen either again.
    pub async fn revoke_all(pool: &sqlx::SqlitePool, list_id: list::Id) -> Result<u64> {
        let result = sqlx::query!(r#"DELETE FROM list_invites WHERE list_id = ?1"#, list_id)
            .execute(pool)
            .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_hashes_to_sixty_four_hex_characters() {
        let h = hash(&Token("hello".into()));

        assert_eq!(h.len(), 64, "the CHECK on the column expects exactly this");
        assert!(
            h.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    #[test]
    fn hashing_is_stable_and_distinguishes() {
        assert_eq!(hash(&Token("a".into())), hash(&Token("a".into())));
        assert_ne!(hash(&Token("a".into())), hash(&Token("b".into())));
    }

    /// The property the storage rests on: what is stored is not what is sent.
    #[test]
    fn the_stored_form_is_not_the_token() {
        let token = Token("a-perfectly-ordinary-token".into());

        assert_ne!(hash(&token), token.0);
        assert!(!hash(&token).contains(&token.0));
    }
}
