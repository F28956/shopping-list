-- created_at is given explicitly: the column default is unixepoch(), so a single
-- multi-row INSERT would stamp every row identically and any ordering assertion on
-- created_at would hold whatever order came back. The offsets are deliberately out
-- of id order, so ordering by created_at is distinguishable from ordering by id.
INSERT INTO users(
    sub,
    email,
    name,
    created_at
) VALUES
    ("google-oauth2|110583927461028374651",                "ana.maria.lopez@example.com",            "Ana María López",          unixepoch() - 7),
    ("google-oauth2|104928374615029384756",                "chen.wei@example.cn",                    "陈伟",                     unixepoch() - 19),
    ("auth0|65f1c0a7b3d2e40012ab34cd",                     "j.vanderberg@example.nl",                "Jan van der Berg",         unixepoch() - 3),
    ("github|8472913",                                     "zoe@example.co.uk",                      "Zoë",                      unixepoch() - 14),
    ("auth0|65f1c0a7b3d2e40012ab35ef",                     "d.sokolov@example.ru",                   "Дмитрий Соколов",          unixepoch() - 20),
    ("google-oauth2|117364829105738291046",                "aoife.nibhraonain@example.ie",           "Aoife Ní Bhraonáin",       unixepoch() - 1),
    ("github|1029384",                                     "kwame.osei.bonsu@example.gh",            "Kwame Osei-Bonsu",         unixepoch() - 11),
    ("auth0|65f1c0a7b3d2e40012ab36aa",                     "thorunn@example.is",                     "Þórunn Jónsdóttir",        unixepoch() - 17),
    ("google-oauth2|108273645019283746501",                "m.alfaisal@example.ae",                  "محمد الفيصل",              unixepoch() - 5),
    ("auth0|65f1c0a7b3d2e40012ab37bb",                     "elodie.moreau.lefevre@example.fr",       "Élodie Moreau-Lefèvre",    unixepoch() - 9),
    ("google-oauth2|119283746501928374650",                "taro.yamada@example.jp",                 "山田 太郎",                unixepoch() - 16),
    ("github|3910284",                                     "minhkhai.nguyen@example.vn",             "Nguyễn Thị Minh Khai",     unixepoch() - 2),
    ("auth0|65f1c0a7b3d2e40012ab38cc",                     "bjorn.akerlund@example.se",              "Björn Åkerlund",           unixepoch() - 13),
    ("google-oauth2|102938475610293847561",                "priya.raghunathan+lists@example.in",     "Priya Raghunathan",        unixepoch() - 8),
    ("auth0|65f1c0a7b3d2e40012ab39dd",                     "sean.osuilleabhain@example.ie",          "Seán Ó Súilleabháin",      unixepoch() - 18),
    ("github|7261839",                                     "m.wisniewska@example.pl",                "Małgorzata Wiśniewska",    unixepoch() - 4),
    ("google-oauth2|113847502938475610293",                "italo.goncalves@example.br",             "Ítalo Gonçalves da Silva", unixepoch() - 12),
    -- signed in, never completed a profile
    ("apple|001923.7f3a9c2b1e4d4f8fa1b2c3d4e5f6a7b8.0930", "hidden-relay-4821@privaterelay.example", NULL,                       unixepoch() - 6),
    -- profile name, but no email shared by the provider
    ("apple|001482.6b2d8a1f0c9e4a17b3c5d7e9f1a2b4c6.1145", NULL,                                     "Sofía Ruiz",               unixepoch() - 15),
    ("google-oauth2|105938271046283910475",                "emeka.okafor@example.ng",                "Emeka Chukwuemeka Okafor", unixepoch() - 10)
;
