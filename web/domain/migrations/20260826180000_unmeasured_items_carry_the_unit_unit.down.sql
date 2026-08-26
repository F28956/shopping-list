-- Back to a missing unit for anything counted singly. Not exact: an item deliberately
-- measured in `unit` is indistinguishable from one that was defaulted into it, which
-- is the information the up migration folded away.
UPDATE items SET unit_id = NULL
WHERE unit_id = (SELECT id FROM units WHERE name = 'unit');

UPDATE item_history SET unit_id = NULL
WHERE unit_id = (SELECT id FROM units WHERE name = 'unit');
