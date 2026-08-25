-- created_at is given explicitly: the column default is unixepoch(), so a single
-- multi-row INSERT would stamp every row identically and any ordering assertion on
-- created_at would hold whatever order came back. The offsets are deliberately out
-- of id order, so ordering by created_at is distinguishable from ordering by id.
INSERT INTO units(
    name,
    created_at
) VALUES
    -- count
    ( "unit",   unixepoch() - 17 ),
    ( "pair",   unixepoch() - 44 ),
    ( "dozen",  unixepoch() - 3  ),
    ( "pack",   unixepoch() - 61 ),
    ( "box",    unixepoch() - 28 ),
    ( "bag",    unixepoch() - 9  ),
    ( "bottle", unixepoch() - 53 ),
    ( "can",    unixepoch() - 22 ),
    ( "jar",    unixepoch() - 70 ),
    ( "tin",    unixepoch() - 12 ),
    ( "tube",   unixepoch() - 38 ),
    ( "sachet", unixepoch() - 5  ),
    ( "roll",   unixepoch() - 66 ),
    ( "bunch",  unixepoch() - 31 ),
    ( "punnet", unixepoch() - 14 ),
    ( "loaf",   unixepoch() - 49 ),
    ( "slice",  unixepoch() - 26 ),
    -- weight
    ( "g",      unixepoch() - 7  ),
    ( "kg",     unixepoch() - 58 ),
    ( "oz",     unixepoch() - 35 ),
    ( "pound",  unixepoch() - 20 ),
    -- volume
    ( "ml",     unixepoch() - 64 ),
    ( "litre",  unixepoch() - 11 ),
    ( "fl oz",  unixepoch() - 42 ),
    ( "pint",   unixepoch() - 1  ),
    ( "gallon", unixepoch() - 56 ),
    ( "tsp",    unixepoch() - 24 ),
    ( "tbsp",   unixepoch() - 47 ),
    ( "cup",    unixepoch() - 15 ),
    -- length
    ( "cm",     unixepoch() - 68 ),
    ( "m",      unixepoch() - 33 )
;
