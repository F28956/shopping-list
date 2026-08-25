-- created_at is given explicitly: the column default is unixepoch(), so a single
-- multi-row INSERT would stamp every row identically and any ordering assertion on
-- created_at would hold whatever order came back. The offsets are deliberately out
-- of id order, so ordering by created_at is distinguishable from ordering by id.
--
-- Notes belong to 4 of the 20 users, unevenly (5, 4, 2, 1), and the remaining 16 own
-- none — an empty note list is the common case, not an edge case.
INSERT INTO notes(
    user_id,
    body,
    created_at
) VALUES
    -- Ana María López: 5
    ((SELECT id FROM users WHERE name = "Ana María López"), "Ask the butcher to trim the fat",       unixepoch() - 31),
    ((SELECT id FROM users WHERE name = "Ana María López"), "Bring the tote bags — no more plastic", unixepoch() - 7),
    ((SELECT id FROM users WHERE name = "Ana María López"), "Yoghurt: the Greek one, not the ""light"" one", unixepoch() - 58),
    ((SELECT id FROM users WHERE name = "Ana María López"), "Recipe needs 1.5 kg, buy two packs",    unixepoch() - 19),
    ((SELECT id FROM users WHERE name = "Ana María López"), "Fruit & veg before the market shuts",   unixepoch() - 44),
    -- 陈伟: 4
    ((SELECT id FROM users WHERE name = "陈伟"), "洗洁精用完了",                                        unixepoch() - 12),
    ((SELECT id FROM users WHERE name = "陈伟"), "Check the bin bags fit the new bin",                unixepoch() - 66),
    ((SELECT id FROM users WHERE name = "陈伟"), "Drinks for Saturday — ask how many are coming",     unixepoch() - 3),
    ((SELECT id FROM users WHERE name = "陈伟"), "Sparkling, not still",                             unixepoch() - 27),
    -- Дмитрий Соколов: 2
    ((SELECT id FROM users WHERE name = "Дмитрий Соколов"), "Мука закончилась",                       unixepoch() - 51),
    ((SELECT id FROM users WHERE name = "Дмитрий Соколов"), "Baking paper, not foil",                 unixepoch() - 22),
    -- Seán Ó Súilleabháin: 1
    ((SELECT id FROM users WHERE name = "Seán Ó Súilleabháin"), "Crisps: the ones without the seasoning sachet", unixepoch() - 38)
;
