-- Every item and list gets a UUID, minted by whichever device created it.
--
-- An item made on a phone with no signal has no `id` -- that is the database's own
-- counter, and only the server turns it. But the operations queued behind that item
-- (tick it off, rename it, delete it) have to name it, so identity cannot wait for
-- the server. The device mints a UUID at the moment of creation and every later
-- operation talks about that; `id` stays the primary key and the foreign key target,
-- because rewriting a schema's worth of integer references would buy nothing.
--
-- The alternative -- temporary ids the server rewrites on sync -- was rejected in
-- docs/offline.md: it needs every queued operation rewritten, and a crash mid-sync
-- leaves a half-rewritten queue.

ALTER TABLE items ADD COLUMN uuid TEXT;
ALTER TABLE lists ADD COLUMN uuid TEXT;

-- Everything that already exists was made before devices minted their own, so the
-- server mints for them. A v4 UUID assembled from SQLite's own randomness: four
-- random bytes, two, then the version nibble `4`, then one of `89ab` for the
-- variant, then the rest. `randomblob` draws from the same source as `random()`,
-- which is seeded per connection from the OS.
UPDATE items SET uuid = lower(
       hex(randomblob(4)) || '-'
    || hex(randomblob(2)) || '-4'
    || substr(hex(randomblob(2)), 2) || '-'
    || substr('89ab', abs(random()) % 4 + 1, 1)
    || substr(hex(randomblob(2)), 2) || '-'
    || hex(randomblob(6))
) WHERE uuid IS NULL;

UPDATE lists SET uuid = lower(
       hex(randomblob(4)) || '-'
    || hex(randomblob(2)) || '-4'
    || substr(hex(randomblob(2)), 2) || '-'
    || substr('89ab', abs(random()) % 4 + 1, 1)
    || substr(hex(randomblob(2)), 2) || '-'
    || hex(randomblob(6))
) WHERE uuid IS NULL;

CREATE UNIQUE INDEX items_by_uuid ON items(uuid);
CREATE UNIQUE INDEX lists_by_uuid ON lists(uuid);

-- The column is nullable, and the triggers below are what make it never be null.
--
-- SQLite cannot add a NOT NULL column to a populated table without a DEFAULT, and
-- the default would have to be constant -- which is precisely what an identity must
-- not be, since every existing row would receive the same one and the UNIQUE index
-- would refuse them all. Rebuilding both tables to earn the keyword would move every
-- row and, with `PRAGMA foreign_keys = ON`, cascade-delete `item_tags` on the way
-- past. Two triggers cost a statement each and move nothing.
--
-- A writer that supplies a UUID keeps it: that is the offline case, where the device
-- minted the identity before the server had heard of the row. A writer that does not
-- gets one from the server, which is the online case and every fixture and console
-- session besides. Either way the row leaves the insert with an identity, so nothing
-- downstream has to ask whether it has one.

CREATE TRIGGER items_are_given_a_uuid AFTER INSERT ON items
WHEN NEW.uuid IS NULL
BEGIN
    UPDATE items SET uuid = lower(
           hex(randomblob(4)) || '-'
        || hex(randomblob(2)) || '-4'
        || substr(hex(randomblob(2)), 2) || '-'
        || substr('89ab', abs(random()) % 4 + 1, 1)
        || substr(hex(randomblob(2)), 2) || '-'
        || hex(randomblob(6))
    ) WHERE id = NEW.id;
END;

CREATE TRIGGER lists_are_given_a_uuid AFTER INSERT ON lists
WHEN NEW.uuid IS NULL
BEGIN
    UPDATE lists SET uuid = lower(
           hex(randomblob(4)) || '-'
        || hex(randomblob(2)) || '-4'
        || substr(hex(randomblob(2)), 2) || '-'
        || substr('89ab', abs(random()) % 4 + 1, 1)
        || substr(hex(randomblob(2)), 2) || '-'
        || hex(randomblob(6))
    ) WHERE id = NEW.id;
END;

-- An identity may not be taken away or swapped once anything has been told it.
-- Queued operations on somebody's phone name the row by this value, and a row whose
-- uuid changed is a row those operations can no longer find.
--
-- `OLD.uuid IS NOT NULL` is what lets the triggers above through: filling in a blank
-- identity is the one write to this column that is not a change of identity.
CREATE TRIGGER items_keep_their_uuid BEFORE UPDATE OF uuid ON items
WHEN OLD.uuid IS NOT NULL AND NEW.uuid IS NOT OLD.uuid
BEGIN SELECT RAISE(ABORT, 'items.uuid cannot change'); END;

CREATE TRIGGER lists_keep_their_uuid BEFORE UPDATE OF uuid ON lists
WHEN OLD.uuid IS NOT NULL AND NEW.uuid IS NOT OLD.uuid
BEGIN SELECT RAISE(ABORT, 'lists.uuid cannot change'); END;
