-- An item added without a unit now carries `unit`, the one that means "counted, not
-- measured". Rows written before that rule have NULL, and the two do not merge: adding
-- `milk` to a list already holding a NULL-unit `Milk` would make a second row, because
-- a missing unit and the `unit` unit are different units.
UPDATE items
SET unit_id = (SELECT id FROM units WHERE name = 'unit')
WHERE unit_id IS NULL
  AND EXISTS (SELECT 1 FROM units WHERE name = 'unit');

-- The memory keys on the same pair, so it gets the same treatment.
UPDATE item_history
SET unit_id = (SELECT id FROM units WHERE name = 'unit')
WHERE unit_id IS NULL
  AND EXISTS (SELECT 1 FROM units WHERE name = 'unit');
