CREATE TABLE notes (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Display text: the model trims it, case preserved. These are a backstop against
    -- writers that do not, not the enforcement point.
    body       TEXT NOT NULL
               CHECK (body <> '' AND body = trim(body) AND length(body) <= 4096),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX notes_by_user ON notes(user_id, id DESC);
