-- How much of it you usually buy.
--
-- The memory already held the unit, so `apples` came back in kilos and then asked how
-- many -- every week, for something bought two kilos at a time every week. Remembering
-- the number is the other half of the same idea, and the one somebody notices.
--
-- Nullable: a name remembered before this knows its unit and not its amount, and
-- guessing one for it would be inventing a fact. `add::resolve` falls back to one.
ALTER TABLE item_history ADD COLUMN amount REAL CHECK (amount IS NULL OR amount > 0);
