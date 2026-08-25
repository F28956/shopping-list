-- created_at and updated_at are given explicitly: the column default is unixepoch(),
-- so a single multi-row INSERT would stamp every row identically and any ordering
-- assertion on either would hold whatever order came back. The offsets are
-- deliberately out of id order, and out of step with each other, so ordering by
-- created_at, by updated_at and by id are three distinguishable orders.
--
-- Every updated_at is more recent than its own created_at, because a list cannot be
-- edited before it exists and a test may reasonably assume that.
INSERT INTO LISTS(
    name,
    owner_id,
    created_at,
    updated_at
) VALUES
    ("Fruit & veg",     (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 70, unixepoch() - 66),
    ("Dairy",           (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 31, unixepoch() - 21),
    ("Bakery",          (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 58, unixepoch() - 33),
    ("Cleaning",        (SELECT id FROM USERS WHERE name = "陈伟"),                      unixepoch() - 12, unixepoch() - 9),
    ("Drinks",          (SELECT id FROM USERS WHERE name = "陈伟"),                      unixepoch() - 44, unixepoch() - 14),
    ("Meat & fish",     (SELECT id FROM USERS WHERE name = "Jan van der Berg"),         unixepoch() - 66, unixepoch() - 64),
    ("Store cupboard",  (SELECT id FROM USERS WHERE name = "Дмитрий Соколов"),           unixepoch() - 19, unixepoch() - 8),
    ("Baking",          (SELECT id FROM USERS WHERE name = "Дмитрий Соколов"),           unixepoch() - 7,  unixepoch() - 1),
    ("Frozen",          (SELECT id FROM USERS WHERE name = "Kwame Osei-Bonsu"),         unixepoch() - 51, unixepoch() - 11),
    ("Toiletries",      (SELECT id FROM USERS WHERE name = "Élodie Moreau-Lefèvre"),    unixepoch() - 27, unixepoch() - 6),
    ("DIY",             (SELECT id FROM USERS WHERE name = "Élodie Moreau-Lefèvre"),    unixepoch() - 38, unixepoch() - 29),
    ("Snacks",          (SELECT id FROM USERS WHERE name = "Seán Ó Súilleabháin"),      unixepoch() - 60, unixepoch() - 15),
    ("Pet & household", (SELECT id FROM USERS WHERE name = "Emeka Chukwuemeka Okafor"), unixepoch() - 22, unixepoch() - 16),
    ("Party",           (SELECT id FROM USERS WHERE name = "Emeka Chukwuemeka Okafor"), unixepoch() - 15, unixepoch() - 10),
    -- Four more for the busiest owner, with no items on them. `for_user` is asserted
    -- to produce a different order for every sortable field in both directions, and
    -- with only three lists there are not enough distinct orders to go round.
    ("Weekend",         (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 47, unixepoch() - 24),
    ("Chemist",         (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 3,  unixepoch() - 2),
    ("Garden",          (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 35, unixepoch() - 5),
    ("Stationery",      (SELECT id FROM USERS WHERE name = "Ana María López"),          unixepoch() - 25, unixepoch() - 19)
;
