-- Which units mean something written without a number in front of them.
--
-- `pint milk` is one pint of milk and everybody knows it. The parser could not read it,
-- because it required a number at one end or the other, so it came out as an item
-- called "pint milk" counted in units.
--
-- Reading *every* unit that way breaks more than it fixes. Half of this table is also
-- the first word of ordinary things to buy: `can opener`, `tin foil`, `box grater`,
-- `tube socks`, `bag clips`, `pound cake`, `roll mat`. Those are not one can of opener.
--
-- So it is per unit, and the ones that stand alone are the measures nobody names a
-- product after. `pound` and `cup` are measures and are still excluded: pound cake and
-- cupcakes are things people write on shopping lists.
ALTER TABLE units ADD COLUMN bare INTEGER NOT NULL DEFAULT 0 CHECK (bare IN (0, 1));

UPDATE units SET bare = 1 WHERE name IN (
    'g', 'kg', 'oz', 'ml', 'litre', 'fl oz', 'pint', 'gallon',
    'tsp', 'tbsp', 'cm', 'm', 'dozen'
);
