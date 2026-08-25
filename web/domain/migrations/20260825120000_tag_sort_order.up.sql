-- Tags become the shape of the list, not just decoration on it.
--
-- Grouping a list by category is what lets it be walked in one pass instead of
-- doubling back, so the order of the groups has to be the order of the shop rather
-- than alphabetical. Perimeter first (produce, bakery, dairy, meat), then the centre
-- aisles, then frozen last so it spends the least time out of the freezer, then the
-- non-food departments.
--
-- Aisle *numbers* would be more precise and are deliberately not used: they differ by
-- branch and change without warning, while categories travel to any shop.

ALTER TABLE tags ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

-- Fresh, walked first.
UPDATE tags SET sort_order = 10 WHERE name = 'produce';
UPDATE tags SET sort_order = 20 WHERE name = 'fruits';
UPDATE tags SET sort_order = 30 WHERE name = 'bakery';
UPDATE tags SET sort_order = 40 WHERE name = 'dairy';
UPDATE tags SET sort_order = 50 WHERE name = 'meat & fish';

-- Centre aisles.
UPDATE tags SET sort_order = 60 WHERE name = 'pantry';
UPDATE tags SET sort_order = 70 WHERE name = 'baking';
UPDATE tags SET sort_order = 80 WHERE name = 'snacks';
UPDATE tags SET sort_order = 90 WHERE name = 'treat';
UPDATE tags SET sort_order = 100 WHERE name = 'drinks';

-- Frozen late, so it is in the trolley for the shortest time.
UPDATE tags SET sort_order = 110 WHERE name = 'frozen';

-- Non-food departments.
UPDATE tags SET sort_order = 120 WHERE name = 'household';
UPDATE tags SET sort_order = 130 WHERE name = 'cleaning';
UPDATE tags SET sort_order = 140 WHERE name = 'toiletries';
UPDATE tags SET sort_order = 150 WHERE name = 'diy';
UPDATE tags SET sort_order = 160 WHERE name = 'party';

-- Shop names and urgency describe *where* or *when*, not what part of the shop, so
-- they sort after everything that does. Left at the 0 default they would sort first.
UPDATE tags SET sort_order = 900 WHERE name IN ('tesco', 'aldi', 'b&q', 'boots', 'urgent');

CREATE INDEX tags_by_sort_order ON tags(sort_order, name);
