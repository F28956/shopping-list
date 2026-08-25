-- created_at is given explicitly: the column default is unixepoch(), so a single
-- multi-row INSERT would stamp every row identically and any ordering assertion on
-- created_at would hold whatever order came back. The offsets are deliberately out
-- of id order, so ordering by created_at is distinguishable from ordering by id.
--
-- Every third item carries a done_at, so a list is part-finished rather than either
-- untouched or complete. `Cake candles` is exempt: it is the fixture's fully sparse
-- row — no unit, no tags, not done — and tests lean on it staying that way. done_at is always more recent than the item's own
-- created_at, and the rest are NULL — ordering by it has to put those last.
INSERT INTO ITEMS(
    list_id,
    name,
    amount,
    unit_id,
    created_at,
    done_at
) VALUES
    -- Fruit & veg
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Apples", 1, (SELECT id FROM UNITS WHERE name = "kg" ), unixepoch() - 1, NULL),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Bananas", 6, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 38, unixepoch() - 37),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Oranges", 2, (SELECT id FROM UNITS WHERE name = "kg" ), unixepoch() - 2, NULL),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Strawberries", 1, (SELECT id FROM UNITS WHERE name = "punnet" ), unixepoch() - 39, NULL),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Blueberries", 2, (SELECT id FROM UNITS WHERE name = "punnet" ), unixepoch() - 3, unixepoch() - 2),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Grapes", 500, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 40, NULL),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Pineapples", 1, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 4, NULL),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Pomegranate", 2, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 41, unixepoch() - 40),
    ((SELECT id FROM LISTS WHERE name = "Fruit & veg"), "Fresh basil", 1, (SELECT id FROM UNITS WHERE name = "bunch" ), unixepoch() - 5, NULL),
    -- Dairy
    ((SELECT id FROM LISTS WHERE name = "Dairy"), "Whole milk", 4, (SELECT id FROM UNITS WHERE name = "pint" ), unixepoch() - 42, NULL),
    ((SELECT id FROM LISTS WHERE name = "Dairy"), "Double cream", 300, (SELECT id FROM UNITS WHERE name = "ml" ), unixepoch() - 6, unixepoch() - 5),
    ((SELECT id FROM LISTS WHERE name = "Dairy"), "Cheddar", 400, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 43, NULL),
    ((SELECT id FROM LISTS WHERE name = "Dairy"), "Greek yoghurt", 500, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 7, NULL),
    ((SELECT id FROM LISTS WHERE name = "Dairy"), "Salted butter", 250, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 44, unixepoch() - 43),
    ((SELECT id FROM LISTS WHERE name = "Dairy"), "Eggs", 1, (SELECT id FROM UNITS WHERE name = "dozen" ), unixepoch() - 8, NULL),
    -- Bakery
    ((SELECT id FROM LISTS WHERE name = "Bakery"), "Sourdough", 1, (SELECT id FROM UNITS WHERE name = "loaf" ), unixepoch() - 45, NULL),
    ((SELECT id FROM LISTS WHERE name = "Bakery"), "Bagels", 1, (SELECT id FROM UNITS WHERE name = "pack" ), unixepoch() - 9, unixepoch() - 8),
    ((SELECT id FROM LISTS WHERE name = "Bakery"), "Croissants", 4, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 46, NULL),
    ((SELECT id FROM LISTS WHERE name = "Bakery"), "Birthday cake", 1, (SELECT id FROM UNITS WHERE name = "box" ), unixepoch() - 10, NULL),
    ((SELECT id FROM LISTS WHERE name = "Bakery"), "Cake candles", 1, NULL, unixepoch() - 47, NULL),
    -- Cleaning
    ((SELECT id FROM LISTS WHERE name = "Cleaning"), "Washing up liquid", 1, (SELECT id FROM UNITS WHERE name = "bottle" ), unixepoch() - 11, NULL),
    ((SELECT id FROM LISTS WHERE name = "Cleaning"), "Kitchen roll", 4, (SELECT id FROM UNITS WHERE name = "roll" ), unixepoch() - 48, NULL),
    ((SELECT id FROM LISTS WHERE name = "Cleaning"), "Bin bags", 1, (SELECT id FROM UNITS WHERE name = "box" ), unixepoch() - 12, unixepoch() - 11),
    ((SELECT id FROM LISTS WHERE name = "Cleaning"), "Bleach", 750, (SELECT id FROM UNITS WHERE name = "ml" ), unixepoch() - 49, NULL),
    ((SELECT id FROM LISTS WHERE name = "Cleaning"), "Dishwasher tablets", 1, (SELECT id FROM UNITS WHERE name = "bag" ), unixepoch() - 13, NULL),
    -- Drinks
    ((SELECT id FROM LISTS WHERE name = "Drinks"), "Orange juice", 2, (SELECT id FROM UNITS WHERE name = "litre" ), unixepoch() - 50, unixepoch() - 49),
    ((SELECT id FROM LISTS WHERE name = "Drinks"), "Sparkling water", 6, (SELECT id FROM UNITS WHERE name = "bottle" ), unixepoch() - 14, NULL),
    ((SELECT id FROM LISTS WHERE name = "Drinks"), "Lager", 12, (SELECT id FROM UNITS WHERE name = "can" ), unixepoch() - 51, NULL),
    ((SELECT id FROM LISTS WHERE name = "Drinks"), "Cider", 2, (SELECT id FROM UNITS WHERE name = "gallon" ), unixepoch() - 15, unixepoch() - 14),
    ((SELECT id FROM LISTS WHERE name = "Drinks"), "Ground coffee", 227, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 52, NULL),
    -- Meat & fish
    ((SELECT id FROM LISTS WHERE name = "Meat & fish"), "Chicken thighs", 1.5, (SELECT id FROM UNITS WHERE name = "kg" ), unixepoch() - 16, NULL),
    ((SELECT id FROM LISTS WHERE name = "Meat & fish"), "Beef mince", 500, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 53, unixepoch() - 52),
    ((SELECT id FROM LISTS WHERE name = "Meat & fish"), "Streaky bacon", 8, (SELECT id FROM UNITS WHERE name = "slice" ), unixepoch() - 17, NULL),
    ((SELECT id FROM LISTS WHERE name = "Meat & fish"), "Salmon fillets", 4, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 54, NULL),
    ((SELECT id FROM LISTS WHERE name = "Meat & fish"), "Prawns", 1, (SELECT id FROM UNITS WHERE name = "pound" ), unixepoch() - 18, unixepoch() - 17),
    -- Store cupboard
    ((SELECT id FROM LISTS WHERE name = "Store cupboard"), "Chopped tomatoes", 4, (SELECT id FROM UNITS WHERE name = "tin" ), unixepoch() - 55, NULL),
    ((SELECT id FROM LISTS WHERE name = "Store cupboard"), "Peanut butter", 1, (SELECT id FROM UNITS WHERE name = "jar" ), unixepoch() - 19, NULL),
    ((SELECT id FROM LISTS WHERE name = "Store cupboard"), "Basmati rice", 2, (SELECT id FROM UNITS WHERE name = "kg" ), unixepoch() - 56, unixepoch() - 55),
    ((SELECT id FROM LISTS WHERE name = "Store cupboard"), "Olive oil", 750, (SELECT id FROM UNITS WHERE name = "ml" ), unixepoch() - 20, NULL),
    ((SELECT id FROM LISTS WHERE name = "Store cupboard"), "Sea salt", 1, (SELECT id FROM UNITS WHERE name = "tsp" ), unixepoch() - 57, NULL),
    ((SELECT id FROM LISTS WHERE name = "Store cupboard"), "Tomato puree", 1, (SELECT id FROM UNITS WHERE name = "tube" ), unixepoch() - 21, unixepoch() - 20),
    -- Baking
    ((SELECT id FROM LISTS WHERE name = "Baking"), "Plain flour", 3, (SELECT id FROM UNITS WHERE name = "pound" ), unixepoch() - 58, NULL),
    ((SELECT id FROM LISTS WHERE name = "Baking"), "Caster sugar", 2, (SELECT id FROM UNITS WHERE name = "cup" ), unixepoch() - 22, NULL),
    ((SELECT id FROM LISTS WHERE name = "Baking"), "Baking powder", 2, (SELECT id FROM UNITS WHERE name = "tbsp" ), unixepoch() - 59, unixepoch() - 58),
    ((SELECT id FROM LISTS WHERE name = "Baking"), "Vanilla extract", 2, (SELECT id FROM UNITS WHERE name = "fl oz" ), unixepoch() - 23, NULL),
    ((SELECT id FROM LISTS WHERE name = "Baking"), "Dried yeast", 3, (SELECT id FROM UNITS WHERE name = "sachet" ), unixepoch() - 60, NULL),
    ((SELECT id FROM LISTS WHERE name = "Baking"), "Dark chocolate", 8, (SELECT id FROM UNITS WHERE name = "oz" ), unixepoch() - 24, unixepoch() - 23),
    -- Frozen
    ((SELECT id FROM LISTS WHERE name = "Frozen"), "Garden peas", 1, (SELECT id FROM UNITS WHERE name = "bag" ), unixepoch() - 61, NULL),
    ((SELECT id FROM LISTS WHERE name = "Frozen"), "Fish fingers", 1, (SELECT id FROM UNITS WHERE name = "box" ), unixepoch() - 25, NULL),
    ((SELECT id FROM LISTS WHERE name = "Frozen"), "Vanilla ice cream", 1, (SELECT id FROM UNITS WHERE name = "litre" ), unixepoch() - 62, unixepoch() - 61),
    ((SELECT id FROM LISTS WHERE name = "Frozen"), "Ice lollies", 1, (SELECT id FROM UNITS WHERE name = "pack" ), unixepoch() - 26, NULL),
    -- Toiletries
    ((SELECT id FROM LISTS WHERE name = "Toiletries"), "Toothpaste", 2, (SELECT id FROM UNITS WHERE name = "tube" ), unixepoch() - 63, NULL),
    ((SELECT id FROM LISTS WHERE name = "Toiletries"), "Shampoo", 500, (SELECT id FROM UNITS WHERE name = "ml" ), unixepoch() - 27, unixepoch() - 26),
    ((SELECT id FROM LISTS WHERE name = "Toiletries"), "Toilet roll", 9, (SELECT id FROM UNITS WHERE name = "roll" ), unixepoch() - 64, NULL),
    ((SELECT id FROM LISTS WHERE name = "Toiletries"), "Razor blades", 1, (SELECT id FROM UNITS WHERE name = "pack" ), unixepoch() - 28, NULL),
    -- DIY
    ((SELECT id FROM LISTS WHERE name = "DIY"), "Masonry paint", 5, (SELECT id FROM UNITS WHERE name = "litre" ), unixepoch() - 65, unixepoch() - 64),
    ((SELECT id FROM LISTS WHERE name = "DIY"), "Sandpaper", 10, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 29, NULL),
    ((SELECT id FROM LISTS WHERE name = "DIY"), "Extension cable", 5, (SELECT id FROM UNITS WHERE name = "m" ), unixepoch() - 66, NULL),
    ((SELECT id FROM LISTS WHERE name = "DIY"), "Copper pipe", 60, (SELECT id FROM UNITS WHERE name = "cm" ), unixepoch() - 30, unixepoch() - 29),
    ((SELECT id FROM LISTS WHERE name = "DIY"), "Work gloves", 1, (SELECT id FROM UNITS WHERE name = "pair" ), unixepoch() - 67, NULL),
    ((SELECT id FROM LISTS WHERE name = "DIY"), "Wood screws", 1, (SELECT id FROM UNITS WHERE name = "box" ), unixepoch() - 31, NULL),
    -- Snacks
    ((SELECT id FROM LISTS WHERE name = "Snacks"), "Salted crisps", 6, (SELECT id FROM UNITS WHERE name = "bag" ), unixepoch() - 68, unixepoch() - 67),
    ((SELECT id FROM LISTS WHERE name = "Snacks"), "Mixed nuts", 300, (SELECT id FROM UNITS WHERE name = "g" ), unixepoch() - 32, NULL),
    ((SELECT id FROM LISTS WHERE name = "Snacks"), "Digestive biscuits", 2, (SELECT id FROM UNITS WHERE name = "pack" ), unixepoch() - 69, NULL),
    -- Pet & household
    ((SELECT id FROM LISTS WHERE name = "Pet & household"), "Dog food", 12, (SELECT id FROM UNITS WHERE name = "can" ), unixepoch() - 33, unixepoch() - 32),
    ((SELECT id FROM LISTS WHERE name = "Pet & household"), "Cat litter", 10, (SELECT id FROM UNITS WHERE name = "kg" ), unixepoch() - 70, NULL),
    ((SELECT id FROM LISTS WHERE name = "Pet & household"), "Light bulbs", 4, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 34, NULL),
    ((SELECT id FROM LISTS WHERE name = "Pet & household"), "Batteries", 1, (SELECT id FROM UNITS WHERE name = "pack" ), unixepoch() - 71, unixepoch() - 70),
    -- Party
    ((SELECT id FROM LISTS WHERE name = "Party"), "Paper cups", 50, (SELECT id FROM UNITS WHERE name = "unit" ), unixepoch() - 35, NULL),
    ((SELECT id FROM LISTS WHERE name = "Party"), "Bunting", 12, (SELECT id FROM UNITS WHERE name = "m" ), unixepoch() - 72, NULL),
    ((SELECT id FROM LISTS WHERE name = "Party"), "Punch bowl mixer", 2, (SELECT id FROM UNITS WHERE name = "gallon" ), unixepoch() - 36, unixepoch() - 35),
    ((SELECT id FROM LISTS WHERE name = "Party"), "Olives", 2, (SELECT id FROM UNITS WHERE name = "jar" ), unixepoch() - 73, NULL),
    ((SELECT id FROM LISTS WHERE name = "Party"), "Party bags", 20, (SELECT id FROM UNITS WHERE name = "bag" ), unixepoch() - 37, NULL)
;
