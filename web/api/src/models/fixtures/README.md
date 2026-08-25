# Fixtures

Seed data for the model tests. Each file is a standalone `.sql` script, embedded by the
`seeds!` macro (`src/models/macros.rs`) and applied by the `pool` fixture
(`src/models/common.rs`):

```rust
#[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
#[future(awt)]
pool: SqlitePool,
```

Paths are relative to `src/models/` and are read at compile time, so a typo fails the
build. The files are applied in the order listed.

## Load order

Rows are linked by **name**, not by hard-coded ids, so the files must be applied in
dependency order:

```
users → lists → units → items → tags
     └→ notes
```

`lists.sql` looks up its owner by `users.name`, `items.sql` looks up its list by
`lists.name` and its unit by `units.name`, `tags.sql` looks up items by `items.name`, and
`notes.sql` looks up its author by `users.name`. Renaming a row in one file means
updating the files downstream of it.

| file | rows | notes |
|---|---|---|
| `users.sql` | 20 | |
| `lists.sql` | 18 | owned by 8 of the 20 users |
| `units.sql` | 31 | count, weight, volume, length |
| `items.sql` | 73 | spread over 14 of the 18 lists |
| `tags.sql` | 21 tags, 150 `item_tags` | associations are set-based, not hand-listed; `created_at` stamped explicitly |
| `notes.sql` | 12 | written by 4 of the 20 users |

## Shape of the data

Lists per owner is deliberately uneven (7, 2, 2, 2, 2, 1, 1, 1), and **12 of the 20 users
own no list at all** — pagination and empty-state paths need a user with nothing.

The busiest owner has seven rather than three because `list::for_user` is asserted to
produce a *different* order for each of its four sortable fields in both directions.
Eight distinct orders do not fit into three rows, so trimming that owner back breaks
`for_user_every_field_changes_the_order` rather than any test about counts.

Notes are shared out the same way: 5, 4, 2, 1, and **16 of the 20 users wrote none**.

| list | owner | items |
|---|---|---|
| Fruit & veg | Ana María López | 9 |
| Dairy | Ana María López | 6 |
| Bakery | Ana María López | 5 |
| Cleaning | 陈伟 | 5 |
| Drinks | 陈伟 | 5 |
| Meat & fish | Jan van der Berg | 5 |
| Store cupboard | Дмитрий Соколов | 6 |
| Baking | Дмитрий Соколов | 6 |
| Frozen | Kwame Osei-Bonsu | 4 |
| Toiletries | Élodie Moreau-Lefèvre | 4 |
| DIY | Élodie Moreau-Lefèvre | 6 |
| Snacks | Seán Ó Súilleabháin | 3 |
| Pet & household | Emeka Chukwuemeka Okafor | 4 |
| Party | Emeka Chukwuemeka Okafor | 5 |
| Weekend | Ana María López | 0 |
| Chemist | Ana María López | 0 |
| Garden | Ana María López | 0 |
| Stationery | Ana María López | 0 |

The last four carry no items on purpose: a list with nothing on it is what
`item::for_list` needs in order to test its empty page.

Every list is themed, and its theme matches its category tag — `Store cupboard` items are
tagged `pantry`, `Pet & household` items are tagged `household`.

## Edge cases

These are the rows that exist *because* they are awkward. Please keep them awkward.

### Users — names

`users.name` is nullable free text, and the fixtures treat it as such:

- **Non-ASCII throughout.** Latin diacritics (`Ana María López`, `Zoë`, `Björn Åkerlund`,
  `Małgorzata Wiśniewska`), Icelandic thorn (`Þórunn Jónsdóttir`), stacked Vietnamese
  diacritics (`Nguyễn Thị Minh Khai`), Cyrillic (`Дмитрий Соколов`), CJK (`陈伟`,
  `山田 太郎`) and RTL Arabic (`محمد الفيصل`). Anything that byte-slices a name, assumes
  one byte per character, or round-trips through a non-UTF-8 layer will break here.
- **Varied word counts.** One part (`Zoë`), two, three (`Aoife Ní Bhraonáin`), four
  (`Nguyễn Thị Minh Khai`). Nothing may assume "first name, last name".
- **Lowercase particles and hyphens.** `Jan van der Berg`, `Ítalo Gonçalves da Silva`,
  `Kwame Osei-Bonsu`, `Élodie Moreau-Lefèvre` — naive title-casing or splitting on `-`
  produces visibly wrong output.
- **A CJK name containing a space** (`山田 太郎`), so "has a space" is not a proxy for
  "is a Western name".

### Users — nulls

Two Apple sign-in users cover the nullable columns, since a provider may withhold either:

| `sub` | `email` | `name` |
|---|---|---|
| `apple\|001923.…0930` | private relay address | `NULL` — signed in, never set a profile name |
| `apple\|001482.…1145` | `NULL` — provider shared no address | `Sofía Ruiz` |

Neither owns a list, so the name-based lookups in `lists.sql` stay resolvable. **If you
give a list to the null-name user, `lists.sql` cannot reference them by name.**

`sub` is `NOT NULL UNIQUE` and uses realistic IdP formats — `google-oauth2|<21 digits>`,
`auth0|<24 hex>`, `github|<int>`, `apple|<hex>.<hex>.<seq>` — so nothing may assume a
`sub` is numeric, short, or free of `|` and `.`.

### Items without tags

Six items have **no rows in `item_tags` at all**, one per list, so an untagged item turns
up in most queries rather than hiding in a corner:

`Fresh basil` (Fruit & veg), `Cake candles` (Bakery), `Sea salt` (Store cupboard),
`Ice lollies` (Frozen), `Sandpaper` (DIY), `Party bags` (Party).

They are excluded by the `WHERE i.name NOT IN (…)` clause on both blanket rules in
`tags.sql`. A `JOIN` onto `item_tags` instead of a `LEFT JOIN` will silently drop these
six items — that is the point of them.

Tag counts per item: **6 items with none, 2 with one, 47 with two, 18 with three.**
The two single-tag items are `Salted crisps` and `Digestive biscuits`; the `Snacks` list
maps to `NULL` in the shop-tag `CASE` ("corner shop"), so its items get a category tag
but no shop tag.

### Items — other

- **`Cake candles` has `unit_id IS NULL`** — the only item with no unit. `units` is
  nullable and the join must be a `LEFT JOIN`. It is also untagged and, unlike every
  third item around it, **not done** — it is the fully sparse row, and
  `item::get_reads_back_a_sparse_item` leans on all three of those staying true.
- **`Chicken thighs` is `1.5 kg`** — `amount` is a `REAL`. Everything else is a whole
  number, so an integer-typed binding passes the other 72 items and fails on this one.
- **Item names are unique across the whole table**, because `tags.sql` matches items by
  name. Adding a duplicate name silently mis-assigns tags rather than erroring.
- **All 31 units are referenced** by at least one item, including the awkward `fl oz`
  (contains a space) and `b&q`-adjacent oddities below.

### Tags

- **`b&q` and `meat & fish` contain an ampersand**, as do the list names `Fruit & veg`
  and `Pet & household` — useful for anything that renders to HTML or builds a query
  string.
- **Every tag carries an emoji**, several of them multi-code-point (`🛠️` is an emoji plus
  a variation selector). Character counts over tag names will not match code-point counts.
- `tags.name` is `UNIQUE COLLATE NOCASE`, so tags are all lowercase and no two differ only
  by case. Units share that constraint — hence one spelling each, `pound` but not `lb`.
- All 21 tags are used by at least one item.

### Timestamps

`users`, `units`, `lists`, `items`, `tags` and `notes` all stamp `created_at` **explicitly**, with
offsets deliberately out of id order. The column default is `unixepoch()`, so a single
multi-row `INSERT` would give every row the same second and ordering by `created_at`
would silently degrade to ordering by `id` — which is exactly the bug the
`*_every_field_changes_the_order` tests exist to catch, so they would stop catching it.

- `lists` also stamps `updated_at`, out of step with `created_at` so the two are
  distinguishable orders, and always more recent than its own row's `created_at`.
- `items` stamps `done_at` on **every third item** (23 of 73 — `Cake candles` would
  have been the 24th, and is deliberately exempt), always more recent than
  that item's `created_at`, with the rest `NULL` — so a list is part-finished, and
  ordering by `done_at` has real values *and* NULLs to place last.

## Not covered

- No list is shared between users; ownership is 1:1, and `list_members` has no fixture —
  see `models::list::Role` for why sharing is not implemented.
- Everything else the fixtures seed now has a model exercising it — `models::tag` covers
  `tags` and `item_tags`, and `item::delete` additionally checks that a deleted item
  takes its `item_tags` rows with it.

## Verifying a change

The fixtures are plain SQLite, so they can be checked without running the test suite:

```sh
rm -f /tmp/fx.db
sqlite3 /tmp/fx.db < migrations/20260816110750_init.up.sql
for f in users lists units items tags notes; do
  sqlite3 /tmp/fx.db < "src/models/fixtures/$f.sql" || echo "FAILED $f"
done
sqlite3 /tmp/fx.db 'PRAGMA foreign_key_check;'
```

Worth re-checking after an edit, because only *some* broken lookups are loud. A subquery
that matches nothing yields `NULL`: against `lists.owner_id` or `items.list_id` that hits
the `NOT NULL` constraint and the load fails visibly, but against the nullable
`items.unit_id` it inserts cleanly and the item just quietly loses its unit. The
name-matching rules in `tags.sql` are quieter still — a name that matches nothing inserts
no row at all. So after renaming anything, check the counts:

```sql
SELECT name FROM items WHERE unit_id IS NULL;   -- expect exactly: Cake candles
SELECT count(*) FROM items WHERE done_at IS NOT NULL;                        -- expect 23
SELECT count(DISTINCT created_at) FROM items;                                -- expect 73
SELECT count(*) FROM items WHERE done_at < created_at;                       -- expect 0
SELECT count(*) FROM items WHERE id NOT IN (SELECT item_id FROM item_tags);  -- expect 6
SELECT count(*) FROM item_tags;                 -- expect 150
SELECT count(*) FROM tags WHERE id NOT IN (SELECT tag_id FROM item_tags);    -- expect 0
```
