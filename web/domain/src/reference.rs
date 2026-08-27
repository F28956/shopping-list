//! The units and tags every client can rely on being there.
//!
//! These are seeded by migration and belong to no user — only `Actor::System` may
//! write them, and no request can produce one. That makes them the one part of this
//! application's data that is the *same everywhere*, which is why a device with no
//! server can still have them.
//!
//! `reference/reference.json` at the root of the repository is that set, written out
//! for the clients to bundle. Nothing here reads it at run time; the server reads its
//! own tables. What this module exists for is the test below, which is the only thing
//! standing between the file and the migrations drifting apart.
//!
//! **The ids matter as much as the names.** An item added on a device with no server
//! carries `unit_id`, and that id is sent as-is when a server finally hears about it.
//! If the file said `kg` was 19 and the server said 19 was `litre`, somebody's two
//! kilograms of apples would arrive as two litres. So the file carries ids and the
//! test checks them.

/// The file, as the clients bundle it.
pub const JSON: &str = include_str!("../../../reference/reference.json");

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sqlx::SqlitePool;

    /// The guard. If a migration adds, renames or reorders a unit or a tag, this fails
    /// until somebody regenerates the file — which is the point, because the clients
    /// ship whatever it last said.
    #[tokio::test]
    async fn the_seed_and_the_file_agree() {
        // Its own database, migrated exactly as production is. The shared `pool`
        // fixture empties these tables so that other tests can control their baseline,
        // which is the opposite of what this one needs.
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();

        let file: Value = serde_json::from_str(JSON).expect("reference.json is not JSON");

        let units: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM units ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();

        let listed = file["units"].as_array().expect("no units in the file");
        assert_eq!(
            listed.len(),
            units.len(),
            "reference.json has {} units and the migrations seed {}",
            listed.len(),
            units.len()
        );

        for (row, (id, name)) in listed.iter().zip(&units) {
            assert_eq!(row["id"].as_i64(), Some(*id), "unit ids differ: {row}");
            assert_eq!(row["name"].as_str(), Some(name.as_str()), "unit names differ");
        }

        let tags: Vec<(i64, String, String, i64)> =
            sqlx::query_as("SELECT id, name, emoji, sort_order FROM tags ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();

        let listed = file["tags"].as_array().expect("no tags in the file");
        assert_eq!(listed.len(), tags.len(), "the file and the migrations disagree on how many tags");

        for (row, (id, name, emoji, sort_order)) in listed.iter().zip(&tags) {
            assert_eq!(row["id"].as_i64(), Some(*id), "tag ids differ: {row}");
            assert_eq!(row["name"].as_str(), Some(name.as_str()), "tag names differ");
            assert_eq!(row["emoji"].as_str(), Some(emoji.as_str()), "tag emoji differ");
            assert_eq!(
                row["sort_order"].as_i64(),
                Some(*sort_order),
                "tag order differs, which is the order every list is grouped in"
            );
        }
    }
}
