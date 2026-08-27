-- Credentials this application issues, as opposed to the ones it merely believes.
--
-- Google's SDK hands a client an ID token and quietly refreshes it, so a Google token
-- can be the bearer on every request for as long as somebody stays signed in. Apple's
-- identity token lasts about ten minutes and cannot be refreshed without asking the
-- person again -- so with Apple the provider's token is a *bootstrap*, exchanged once
-- for something this server issued and can take back.
--
-- Only the hash is stored. The token itself exists in the client's keychain and in the
-- Authorization header, and nowhere else -- a leaked backup hands out nothing, exactly
-- as it does for `list_invites`.
CREATE TABLE app_sessions (
    -- SHA-256 of the token, lowercase hex. Unsalted on purpose: the token is 256 bits
    -- of randomness, so there is no dictionary to defend against, and an unsalted hash
    -- is what lets a lookup be a primary-key hit rather than a scan.
    token_hash   TEXT PRIMARY KEY
                 CHECK (length(token_hash) = 64),
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- What the person signed in with. Kept for the log line when somebody asks why a
    -- session exists, and so revoking "everything Apple" is a query rather than a
    -- guess.
    provider     TEXT NOT NULL CHECK (provider <> ''),
    created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    -- Moved forward on use, and what the expiry is measured from. A phone that is
    -- opened every week never signs out; one left in a drawer for three months does.
    last_used_at INTEGER NOT NULL DEFAULT (unixepoch())
) WITHOUT ROWID;

CREATE INDEX app_sessions_by_user ON app_sessions(user_id, last_used_at DESC);
