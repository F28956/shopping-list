-- The memory had room for one tag, and attaching a second overwrote the first.
--
-- An item filed under both a shop and a category -- `aldi` and `produce` -- came
-- back with whichever was attached last, which looked like the memory forgetting at
-- random. One column was the whole reason: `item_history.tag_id`.
CREATE TABLE item_history_tags (
    list_id INTEGER NOT NULL,
    -- The same normalised key `item_history` uses: trimmed and lowercased in Rust,
    -- because SQLite's lower() folds ASCII only.
    name    TEXT    NOT NULL,
    tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,

    PRIMARY KEY (list_id, name, tag_id),
    -- Forgetting an item forgets what it was filed under, without a second delete.
    FOREIGN KEY (list_id, name) REFERENCES item_history(list_id, name) ON DELETE CASCADE
) WITHOUT ROWID;

-- What was remembered under the old rule is still true, just no longer alone.
INSERT INTO item_history_tags (list_id, name, tag_id)
SELECT list_id, name, tag_id FROM item_history WHERE tag_id IS NOT NULL;

-- Dropped rather than left in place. A column nothing reads is a column somebody
-- writes to by mistake.
ALTER TABLE item_history DROP COLUMN tag_id;
