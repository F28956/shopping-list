CREATE TABLE users (
    id         INTEGER PRIMARY KEY,
    -- `sub` is the provider's identity for this person. It is matched exactly and
    -- never case-folded: two subs differing only in case are two people, so this
    -- column deliberately has no COLLATE NOCASE.
    sub        TEXT NOT NULL UNIQUE
               CHECK (sub <> '' AND sub = trim(sub) AND length(sub) <= 255),
    -- The model normalises addresses (trimmed, lowercased) before writing; this is a
    -- backstop against writers that do not, not the enforcement point. SQLite cannot
    -- case-fold beyond ASCII, so `email = lower(email)` is deliberately not asserted.
    email      TEXT CHECK (email IS NULL
                           OR (email <> '' AND email = trim(email) AND length(email) <= 320)),
    -- Display text: trimmed, case preserved. 320/128 are RFC 5321's path limit and a
    -- generous bound on a person's name.
    name       TEXT CHECK (name IS NULL
                           OR (name <> '' AND name = trim(name) AND length(name) <= 128)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE lists (
    id         INTEGER PRIMARY KEY,
    -- Display text: the model trims it, case preserved. A backstop against writers
    -- that do not, not the enforcement point. Not UNIQUE, and not unique per owner
    -- either: two lists may share a name, so only `id` identifies one.
    name       TEXT NOT NULL
               CHECK (name <> '' AND name = trim(name) AND length(name) <= 128),
    owner_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX lists_by_user ON lists(owner_id, id DESC);

CREATE TABLE list_members (
    list_id   INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role      TEXT NOT NULL DEFAULT 'editor' CHECK (role IN ('owner','editor','viewer')),
    added_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (list_id, user_id)
) WITHOUT ROWID;
CREATE INDEX list_members_by_user ON list_members(user_id, list_id);

CREATE TABLE units (
    id         INTEGER PRIMARY KEY,
    -- The model normalises names (trimmed, lowercased) before writing; these are a
    -- backstop against writers that do not, not the enforcement point. SQLite cannot
    -- case-fold beyond ASCII, so `name = lower(name)` is deliberately not asserted.
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE
               CHECK (name <> '' AND name = trim(name) AND length(name) <= 64),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE items (
    id         INTEGER NOT NULL PRIMARY KEY,
    list_id    INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    -- Display text: the model trims it, case preserved. A backstop against writers
    -- that do not, not the enforcement point.
    name       TEXT NOT NULL
               CHECK (name <> '' AND name = trim(name) AND length(name) <= 128),
    amount     REAL NOT NULL DEFAULT 1 CHECK (amount > 0),
    unit_id    INTEGER REFERENCES units(id) ON DELETE RESTRICT,
    done_at    INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX items_by_list ON items(list_id, created_at);

CREATE TABLE tags (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE COLLATE NOCASE,
    colour TEXT,
    emoji  TEXT
);

CREATE TABLE item_tags (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (item_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX item_tags_by_tag ON item_tags(tag_id, item_id);
