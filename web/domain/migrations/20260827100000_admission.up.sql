-- Who may sign in, as data rather than as an environment variable.
--
-- `ALLOWED_EMAILS` is read once at boot, so changing who may use a server is a
-- redeploy — and the person running a server is not always the person holding a
-- shell. These tables move the decision to where it belongs, and `identity` keeps
-- checking it on every request so that removing somebody still takes effect at once.
CREATE TABLE admitted_emails (
    -- Normalised lowercase, as `Admission` already compares.
    email    TEXT PRIMARY KEY CHECK (email <> '' AND email = lower(email)),
    -- Bound on first successful sign-in, and the reason this column exists.
    --
    -- A provider address is not stable; the subject is. If admission were checked by
    -- address for ever, somebody changing the address on their Google account would
    -- be locked out of a server holding their own lists. So: checked by address until
    -- there is a user, bound here on first contact, and checked by user afterwards.
    -- The address stays as the label a person reads.
    user_id  INTEGER REFERENCES users(id) ON DELETE SET NULL,
    -- Null where the row was seeded from configuration rather than added by anybody.
    -- `ALLOWED_EMAILS` names no author, and inventing one would be a lie in a column
    -- somebody will later read as an audit trail.
    added_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    -- "mum", so that a list of addresses stays readable.
    note     TEXT
) WITHOUT ROWID;

CREATE INDEX admitted_emails_by_user ON admitted_emails(user_id);

-- Whoever may administer the server: admit people, withdraw them, and promote others.
--
-- A flag rather than a table, so there can be several and they are equal. The
-- alternative — an owner who cannot be demoted by anyone they promoted — is a
-- hierarchy nobody has asked for.
--
-- It is not a data role. An owner decides who may use the machine and has no more
-- access to anybody's lists than any other person does.
ALTER TABLE users ADD COLUMN is_owner INTEGER NOT NULL DEFAULT 0;

-- One row, holding what is true of this server rather than of anybody on it.
CREATE TABLE server (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    -- `Admission::Anyone`, which is a legitimate thing to want and should be said
    -- deliberately. Stored rather than spelled `*` in a variable, so that turning it
    -- on is an action somebody took and not a character somebody typed.
    admits_anyone INTEGER NOT NULL DEFAULT 0,
    -- Set once the first person claims the server. Until then there is no owner and
    -- the claim code in the log is what gets you in — see `service::admission`.
    claimed_at    INTEGER
);

INSERT INTO server (id) VALUES (1);
