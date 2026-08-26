-- Which tag decides where an item sits, per person, per list.
--
-- One global order cannot serve every list. The categories run in the order a
-- supermarket is walked (produce 10 ... party 160) and the shop and priority tags all
-- sit at 900, so an item tagged `aldi` and `produce` always groups under produce and
-- never under the shop -- and `urgent` can never come first anywhere.
--
-- Per person as well as per list, because two people sharing a list are not
-- necessarily walking the same route.
CREATE TABLE list_tag_order (
    list_id    INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tag_id     INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    -- Where this tag falls for this person on this list. Dense and zero-based, but
    -- nothing depends on that: only the relative order is read.
    position   INTEGER NOT NULL CHECK (position >= 0),
    -- When this person first set an order here. Read to answer "whose order does
    -- somebody who has not set one inherit" -- the earliest, so a list has a settled
    -- shape as soon as one person gives it one.
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),

    PRIMARY KEY (list_id, user_id, tag_id)
) WITHOUT ROWID;

-- The lookup is always "this list, this person, in order".
CREATE INDEX list_tag_order_by_person ON list_tag_order(list_id, user_id, position);
