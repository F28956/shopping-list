-- Back to one tag, which means choosing one: the lowest id, arbitrarily, because
-- the information to choose better was never stored.
ALTER TABLE item_history ADD COLUMN tag_id INTEGER REFERENCES tags(id) ON DELETE SET NULL;

UPDATE item_history
SET tag_id = (
    SELECT min(tag_id) FROM item_history_tags t
    WHERE t.list_id = item_history.list_id AND t.name = item_history.name
);

DROP TABLE item_history_tags;
