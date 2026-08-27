-- Which provider identities belong to which person.
--
-- `users.sub` was the whole answer while there was one provider. There are now two:
-- the Apple clients sign in with Apple and Android signs in with Google, and the same
-- human arrives with a different subject depending on which device is in their hand.
-- One column cannot hold two answers.
--
-- Keyed on (provider, subject) because a subject is only unique within the provider
-- that issued it -- Apple and Google could in principle mint the same string, and it
-- would mean two different people.
CREATE TABLE user_identities (
    provider   TEXT NOT NULL
               CHECK (provider <> '' AND provider = trim(provider) AND length(provider) <= 32),
    -- The provider's identity for this person. Matched exactly and never case-folded,
    -- for the same reason `users.sub` is not: two subjects differing only in case are
    -- two people.
    subject    TEXT NOT NULL
               CHECK (subject <> '' AND subject = trim(subject) AND length(subject) <= 255),
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (provider, subject)
) WITHOUT ROWID;

CREATE INDEX user_identities_by_user ON user_identities(user_id);

-- Everybody who already exists signed in with Google, because it was the only way in.
INSERT INTO user_identities (provider, subject, user_id, created_at)
SELECT 'google', sub, id, created_at FROM users;

-- `users.sub` stays, and stays unique. It is the identity the account was created
-- with -- the one `User::get(Lookup::Sub)` still answers to -- and dropping it would
-- rewrite a table's worth of history to save a column. Every identity, including that
-- first one, also has a row above; the table is the answer to "who is this", and the
-- column is a record of who they were when they arrived.
