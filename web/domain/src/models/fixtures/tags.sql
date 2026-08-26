-- created_at is given explicitly: the column default is unixepoch(), so a single
-- multi-row INSERT would stamp every row identically and any ordering assertion on
-- created_at would hold whatever order came back. The offsets are deliberately out
-- of id order, so ordering by created_at is distinguishable from ordering by id.
-- sort_order is given for the same reason as created_at, and is deliberately neither
-- id order nor created_at order: it is the order a shop is walked, and a test that
-- cannot tell it apart from the others is not testing it.
INSERT INTO TAGS(
    name,
    colour,
    emoji,
    created_at,
    sort_order
) VALUES
    -- shops
    ( "tesco", "#00539F", "🛒", unixepoch() - 14, 900 ),
    ( "aldi", "#24A9E1", "🛒", unixepoch() - 47, 900 ),
    ( "b&q", "#FFA500", "🛠️", unixepoch() - 3, 900 ),
    ( "boots", "#005EB8", "💊", unixepoch() - 71, 900 ),
    -- categories
    ( "produce", "#4CAF50", "🥬", unixepoch() - 26, 10 ),
    ( "fruits", "#008000", "🍎", unixepoch() - 58, 20 ),
    ( "dairy", "#FFF3C4", "🧀", unixepoch() - 9, 40 ),
    ( "bakery", "#C68642", "🥖", unixepoch() - 35, 30 ),
    ( "meat & fish", "#B03A2E", "🥩", unixepoch() - 66, 50 ),
    ( "frozen", "#7FDBFF", "🧊", unixepoch() - 18, 110 ),
    ( "drinks", "#8E44AD", "🥤", unixepoch() - 52, 100 ),
    ( "pantry", "#8D6E63", "🥫", unixepoch() - 5, 60 ),
    ( "baking", "#F5CBA7", "🧁", unixepoch() - 41, 70 ),
    ( "snacks", "#E67E22", "🍿", unixepoch() - 74, 80 ),
    ( "cleaning", "#00BCD4", "🧽", unixepoch() - 22, 130 ),
    ( "toiletries", "#EC407A", "🪥", unixepoch() - 60, 140 ),
    ( "household", "#607D8B", "🏠", unixepoch() - 12, 120 ),
    ( "diy", "#795548", "🔩", unixepoch() - 44, 150 ),
    ( "party", "#FF4081", "🎉", unixepoch() - 30, 160 ),
    -- workflow
    ( "urgent", "#D32F2F", "⚡", unixepoch() - 68, 900 ),
    ( "treat", "#FFD700", "⭐", unixepoch() - 1, 90 )
;

-- category tag for every item on a list
INSERT INTO ITEM_TAGS( item_id, tag_id )
SELECT i.id, t.id
FROM ITEMS i
JOIN LISTS l ON l.id = i.list_id
JOIN TAGS  t ON t.name = (
    CASE l.name
        WHEN "Fruit & veg"     THEN "produce"
        WHEN "Dairy"           THEN "dairy"
        WHEN "Bakery"          THEN "bakery"
        WHEN "Cleaning"        THEN "cleaning"
        WHEN "Drinks"          THEN "drinks"
        WHEN "Meat & fish"     THEN "meat & fish"
        WHEN "Store cupboard"  THEN "pantry"
        WHEN "Baking"          THEN "baking"
        WHEN "Frozen"          THEN "frozen"
        WHEN "Toiletries"      THEN "toiletries"
        WHEN "DIY"             THEN "diy"
        WHEN "Snacks"          THEN "snacks"
        WHEN "Pet & household" THEN "household"
        WHEN "Party"           THEN "party"
    END
)
WHERE i.name NOT IN (
    -- deliberately left with no tags at all
    "Fresh basil", "Cake candles", "Sea salt",
    "Ice lollies", "Sandpaper", "Party bags"
);

-- shop tag for every item on a list
INSERT INTO ITEM_TAGS( item_id, tag_id )
SELECT i.id, t.id
FROM ITEMS i
JOIN LISTS l ON l.id = i.list_id
JOIN TAGS  t ON t.name = (
    CASE l.name
        WHEN "Cleaning"        THEN "aldi"
        WHEN "Toiletries"      THEN "boots"
        WHEN "DIY"             THEN "b&q"
        WHEN "Pet & household" THEN "aldi"
        WHEN "Snacks"          THEN NULL   -- corner shop, untagged
        ELSE "tesco"
    END
)
WHERE i.name NOT IN (
    -- deliberately left with no tags at all
    "Fresh basil", "Cake candles", "Sea salt",
    "Ice lollies", "Sandpaper", "Party bags"
);

-- fruit on the produce list
INSERT INTO ITEM_TAGS( item_id, tag_id )
SELECT i.id, (SELECT id FROM TAGS WHERE name = "fruits")
FROM ITEMS i
WHERE i.name IN (
    "Apples", "Bananas", "Oranges", "Strawberries",
    "Blueberries", "Grapes", "Pineapples", "Pomegranate"
);

-- odds and ends
INSERT INTO ITEM_TAGS( item_id, tag_id )
SELECT i.id, (SELECT id FROM TAGS WHERE name = "urgent")
FROM ITEMS i
WHERE i.name IN ( "Whole milk", "Eggs", "Toilet roll", "Bin bags", "Dog food", "Batteries" );

INSERT INTO ITEM_TAGS( item_id, tag_id )
SELECT i.id, (SELECT id FROM TAGS WHERE name = "treat")
FROM ITEMS i
WHERE i.name IN ( "Dark chocolate", "Vanilla ice cream", "Croissants", "Mixed nuts", "Birthday cake" );
