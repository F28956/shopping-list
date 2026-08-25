INSERT INTO TAGS(
    name,
    colour,
    emoji
) VALUES
    -- shops
    ( "tesco", "#00539F", "🛒" ),
    ( "aldi", "#24A9E1", "🛒" ),
    ( "b&q", "#FFA500", "🛠️" ),
    ( "boots", "#005EB8", "💊" ),
    -- categories
    ( "produce", "#4CAF50", "🥬" ),
    ( "fruits", "#008000", "🍎" ),
    ( "dairy", "#FFF3C4", "🧀" ),
    ( "bakery", "#C68642", "🥖" ),
    ( "meat & fish", "#B03A2E", "🥩" ),
    ( "frozen", "#7FDBFF", "🧊" ),
    ( "drinks", "#8E44AD", "🥤" ),
    ( "pantry", "#8D6E63", "🥫" ),
    ( "baking", "#F5CBA7", "🧁" ),
    ( "snacks", "#E67E22", "🍿" ),
    ( "cleaning", "#00BCD4", "🧽" ),
    ( "toiletries", "#EC407A", "🪥" ),
    ( "household", "#607D8B", "🏠" ),
    ( "diy", "#795548", "🔩" ),
    ( "party", "#FF4081", "🎉" ),
    -- workflow
    ( "urgent", "#D32F2F", "⚡" ),
    ( "treat", "#FFD700", "⭐" )
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
