-- Which queued operations the server has already applied.
--
-- `POST /api/sync` promises that sending the same batch twice does nothing the second
-- time. That promise cannot rest on the operations themselves being idempotent: most
-- of them are -- `add` is, `setDone` is, `delete` is -- but "most" is not a promise,
-- and a client that resends because an answer was lost deserves a better one than
-- "probably fine".
--
-- So each operation carries a UUID it minted, and this table is the memory of it.
--
-- Keyed by the operation alone, not by (operation, person). Two people cannot mint the
-- same UUID, and if they somehow did, the second one is a collision we would rather
-- refuse than apply.
CREATE TABLE applied_operations (
    id         TEXT PRIMARY KEY
               CHECK (id <> '' AND id = trim(id) AND length(id) = 36),
    -- Who sent it. Not part of the key -- see above -- but worth having when somebody
    -- asks why a row changed.
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- What it did, for the same reason.
    kind       TEXT NOT NULL CHECK (kind <> ''),
    -- When the server applied it. Deliberately not when the device says it happened:
    -- that belongs to the operation, and this column answers "when did we act", which
    -- is a question about the server.
    applied_at INTEGER NOT NULL DEFAULT (unixepoch())
) WITHOUT ROWID;

CREATE INDEX applied_operations_by_user ON applied_operations(user_id, applied_at DESC);
