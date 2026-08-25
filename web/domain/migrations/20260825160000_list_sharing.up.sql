-- Sharing a list, and moving its memory onto it.
--
-- `list_members` was mapped from the beginning and never used; this is the migration
-- that gives it teeth, alongside the invites that populate it.

-- An invitation is a bearer credential: whoever holds the link gets the role baked
-- into it. Only the hash is stored, so a leaked backup does not hand out access —
-- the raw token exists in the URL and nowhere else.
CREATE TABLE list_invites (
    token_hash TEXT    PRIMARY KEY NOT NULL CHECK (length(token_hash) = 64),
    list_id    INTEGER NOT NULL REFERENCES lists(id)  ON DELETE CASCADE,
    -- `owner` is deliberately absent: ownership transfers deliberately, not by link.
    role       TEXT    NOT NULL CHECK (role IN ('editor', 'viewer')),
    created_by INTEGER NOT NULL REFERENCES users(id)  ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL,
    used_at    INTEGER
) WITHOUT ROWID;

CREATE INDEX list_invites_by_list ON list_invites(list_id);

-- The memory moves from the person to the list, so a household shares one.
--
-- Rebuilt from `items` rather than translated: a row keyed by user cannot be assigned
-- to one of their lists after the fact. Anything learned from an item since deleted
-- is lost, which is the price of the move.
DROP TABLE item_history;

CREATE TABLE item_history (
    list_id      INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    -- The key: trimmed and lowercased, so `milk`, `Milk` and `MILK` are one memory.
    name         TEXT    NOT NULL CHECK (name <> '' AND name = trim(name)),
    -- What to show back — the spelling last used, by whoever used it.
    display      TEXT    NOT NULL CHECK (display <> '' AND display = trim(display)),
    -- SET NULL rather than RESTRICT, unlike items.unit_id: a unit in use on a real
    -- list should block its deletion, but one merely remembered should not.
    unit_id      INTEGER REFERENCES units(id) ON DELETE SET NULL,
    tag_id       INTEGER REFERENCES tags(id)  ON DELETE SET NULL,
    uses         INTEGER NOT NULL DEFAULT 1 CHECK (uses > 0),
    last_used_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (list_id, name)
) WITHOUT ROWID;

CREATE INDEX item_history_by_recency ON item_history(list_id, last_used_at DESC);

INSERT INTO item_history (list_id, name, display, unit_id, uses, last_used_at)
SELECT
    i.list_id,
    lower(trim(i.name)),
    max(i.name),
    (
        SELECT i2.unit_id
        FROM items i2
        WHERE i2.list_id = i.list_id
          AND lower(trim(i2.name)) = lower(trim(i.name))
          AND i2.unit_id IS NOT NULL
        ORDER BY i2.created_at DESC
        LIMIT 1
    ),
    count(*),
    max(i.created_at)
FROM items i
WHERE trim(i.name) <> ''
GROUP BY i.list_id, lower(trim(i.name));
