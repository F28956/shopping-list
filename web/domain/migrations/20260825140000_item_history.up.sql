-- What a person buys, as opposed to what is on a list right now.
--
-- These were the same thing until now: suggestions were derived from live `items`
-- rows, so deleting an item deleted the memory of it — and "clear done", the natural
-- end-of-shop action, wiped the lot. A record of habits has to outlive the lists it
-- was gathered from.
--
-- It also gives the remembered unit and category somewhere to live, which is what
-- lets a re-added item arrive already measured and already filed.
CREATE TABLE item_history (
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The key: trimmed and lowercased, so `milk`, `Milk` and `MILK` are one memory.
    name         TEXT    NOT NULL CHECK (name <> '' AND name = trim(name)),
    -- What to show back — the spelling they last used.
    display      TEXT    NOT NULL CHECK (display <> '' AND display = trim(display)),
    -- SET NULL rather than RESTRICT, deliberately unlike items.unit_id: a unit in use
    -- on a real list should block its deletion, but one merely remembered should not.
    -- No convenience gets to veto an administrative change.
    unit_id      INTEGER REFERENCES units(id) ON DELETE SET NULL,
    tag_id       INTEGER REFERENCES tags(id)  ON DELETE SET NULL,
    uses         INTEGER NOT NULL DEFAULT 1 CHECK (uses > 0),
    last_used_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, name)
) WITHOUT ROWID;

CREATE INDEX item_history_by_recency ON item_history(user_id, last_used_at DESC);

-- Backfill, so nothing anyone has already typed is thrown away.
--
-- SQLite's lower() folds ASCII only, which is why normalisation lives in Rust from
-- here on. For a one-off backfill it is close enough: the worst case is two entries
-- for one non-ASCII name, and the next use merges them.
INSERT INTO item_history (user_id, name, display, unit_id, uses, last_used_at)
SELECT
    l.owner_id,
    lower(trim(i.name)),
    max(i.name),
    (
        SELECT i2.unit_id
        FROM items i2
        JOIN lists l2 ON l2.id = i2.list_id
        WHERE l2.owner_id = l.owner_id
          AND lower(trim(i2.name)) = lower(trim(i.name))
          AND i2.unit_id IS NOT NULL
        ORDER BY i2.created_at DESC
        LIMIT 1
    ),
    count(*),
    max(i.created_at)
FROM items i
JOIN lists l ON l.id = i.list_id
WHERE trim(i.name) <> ''
GROUP BY l.owner_id, lower(trim(i.name));
