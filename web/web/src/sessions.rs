//! Sessions, stored in the same SQLite file as everything else.
//!
//! `tower-sessions-sqlx-store` exists, but it is built against sqlx 0.8 while this
//! workspace is on 0.9 — its `Pool<Sqlite>` is a different type from ours, so its
//! store cannot take our pool. Pinning the whole workspace back a major version to
//! borrow eighty lines is the worse trade, so here are the eighty lines.
//!
//! What this replaces is `MemoryStore`, which signed every user out on every restart.

use async_trait::async_trait;
use domain::service::Ctx;
use time::OffsetDateTime;
use tower_sessions::SessionStore;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{Error, Result};

/// A session store over the application's own pool.
#[derive(Debug, Clone)]
pub struct SqliteSessions {
    db: sqlx::SqlitePool,
}

impl SqliteSessions {
    pub fn new(ctx: &Ctx) -> Self {
        Self { db: ctx.db.clone() }
    }

    /// Creates the table if it is not there.
    ///
    /// Deliberately not a migration in `domain`: the schema of a session row belongs
    /// to whichever transport keeps sessions, and the API and MCP do not.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id         TEXT PRIMARY KEY NOT NULL,
                data       BLOB NOT NULL,
                expires_at INTEGER NOT NULL
            ) WITHOUT ROWID
            "#,
        )
        .execute(&self.db)
        .await
        .map_err(backend)?;

        // Expired rows are swept by whoever looks at them next; this index is what
        // makes a periodic bulk sweep cheap when one is added.
        sqlx::query("CREATE INDEX IF NOT EXISTS sessions_by_expiry ON sessions(expires_at)")
            .execute(&self.db)
            .await
            .map_err(backend)?;

        Ok(())
    }
}

fn backend(e: sqlx::Error) -> Error {
    Error::Backend(e.to_string())
}

fn encode(record: &Record) -> Result<Vec<u8>> {
    rmp_serde::to_vec(record).map_err(|e| Error::Encode(e.to_string()))
}

fn decode(bytes: &[u8]) -> Result<Record> {
    rmp_serde::from_slice(bytes).map_err(|e| Error::Decode(e.to_string()))
}

// tower-sessions declares SessionStore with #[async_trait], so the impl must match
#[async_trait]
impl SessionStore for SqliteSessions {
    async fn save(&self, record: &Record) -> Result<()> {
        let id = record.id.to_string();
        let data = encode(record)?;
        let expires_at = record.expiry_date.unix_timestamp();

        sqlx::query(
            r#"
            INSERT INTO sessions (id, data, expires_at) VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET data = ?2, expires_at = ?3
            "#,
        )
        .bind(&id)
        .bind(&data)
        .bind(expires_at)
        .execute(&self.db)
        .await
        .map_err(backend)?;

        Ok(())
    }

    async fn load(&self, session_id: &Id) -> Result<Option<Record>> {
        let id = session_id.to_string();
        let now = OffsetDateTime::now_utc().unix_timestamp();

        // Expiry is enforced here rather than by a sweeper, so an expired session is
        // never loaded even if its row is still on disk.
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT data FROM sessions WHERE id = ?1 AND expires_at > ?2")
                .bind(&id)
                .bind(now)
                .fetch_optional(&self.db)
                .await
                .map_err(backend)?;

        row.map(|(data,)| decode(&data)).transpose()
    }

    async fn delete(&self, session_id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?1")
            .bind(session_id.to_string())
            .execute(&self.db)
            .await
            .map_err(backend)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use domain::models::pool;
    use rstest::rstest;
    use sqlx::SqlitePool;
    use time::Duration;

    use super::*;

    async fn store(pool: SqlitePool) -> SqliteSessions {
        let store = SqliteSessions::new(&Ctx::new(pool));
        store.migrate().await.expect("session table");
        store
    }

    fn record(expires_in: Duration) -> Record {
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), serde_json::json!(42));
        Record {
            id: Id::default(),
            data,
            expiry_date: OffsetDateTime::now_utc() + expires_in,
        }
    }

    #[rstest]
    #[tokio::test]
    async fn a_session_round_trips(#[future(awt)] pool: SqlitePool) {
        let store = store(pool).await;
        let record = record(Duration::days(7));

        store.save(&record).await.unwrap();
        let loaded = store
            .load(&record.id)
            .await
            .unwrap()
            .expect("saved session");

        assert_eq!(loaded.id, record.id);
        assert_eq!(
            loaded.data, record.data,
            "the payload survives the round trip"
        );
    }

    /// Saving the same id twice updates rather than colliding — that is what happens
    /// on every request that touches the session.
    #[rstest]
    #[tokio::test]
    async fn saving_twice_updates(#[future(awt)] pool: SqlitePool) {
        let store = store(pool).await;
        let mut record = record(Duration::days(7));
        store.save(&record).await.unwrap();

        record.data.insert("user_id".into(), serde_json::json!(43));
        store.save(&record).await.unwrap();

        let loaded = store.load(&record.id).await.unwrap().unwrap();
        assert_eq!(loaded.data["user_id"], serde_json::json!(43));
    }

    /// Expiry is enforced on load, so an expired session is never handed back even
    /// though its row is still on disk.
    #[rstest]
    #[tokio::test]
    async fn an_expired_session_does_not_load(#[future(awt)] pool: SqlitePool) {
        let store = store(pool).await;
        let record = record(Duration::seconds(-1));

        store.save(&record).await.unwrap();

        assert!(store.load(&record.id).await.unwrap().is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn delete_removes_it(#[future(awt)] pool: SqlitePool) {
        let store = store(pool).await;
        let record = record(Duration::days(7));
        store.save(&record).await.unwrap();

        store.delete(&record.id).await.unwrap();

        assert!(store.load(&record.id).await.unwrap().is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn an_unknown_session_is_none(#[future(awt)] pool: SqlitePool) {
        let store = store(pool).await;

        assert!(store.load(&Id::default()).await.unwrap().is_none());
    }

    /// The point of moving off MemoryStore: a new store over the same database sees
    /// sessions the old one wrote, which is what a restart looks like.
    #[rstest]
    #[tokio::test]
    async fn sessions_survive_a_restart(#[future(awt)] pool: SqlitePool) {
        let record = record(Duration::days(7));
        {
            let before = store(pool.clone()).await;
            before.save(&record).await.unwrap();
        }

        let after = SqliteSessions::new(&Ctx::new(pool));

        assert!(
            after.load(&record.id).await.unwrap().is_some(),
            "a restart signed the user out"
        );
    }
}
