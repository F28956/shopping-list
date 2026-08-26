use time::OffsetDateTime;

use super::{Error, Result};
use super::{OffsetPage, OrderBy, Paging};
use super::{item, list};

// Scaffold Id, Name, Colour, Emoji and CreatedAt
i64!(Id);
i64!(SortOrder);
string!(Name, Colour, Emoji);
timestamp!(CreatedAt);

// A tag name is the dedupe key — `tags.name` is `UNIQUE COLLATE NOCASE`, so `Dairy`,
// `dairy ` and `DAIRY` are one tag
normalized!(Name);
// Presentation, not keys: `#00539F` is written in uppercase hex and case-folding an
// emoji is meaningless at best, so these only lose their padding
trimmed!(Colour, Emoji);

/// The longest name `tags.name` accepts, in characters — keep in step with the
/// `CHECK` in the init migration. Anything longer is [`Error::InvalidInput`].
pub const MAX_NAME: usize = 64;
/// Room for `#RRGGBB` several times over, so a caller may store `rgb(0 83 159)` or a
/// CSS colour keyword instead of hex.
pub const MAX_COLOUR: usize = 32;
/// Characters, not emoji: `🛠️` is a base plus a variation selector, and a flag or a
/// family is several code points more. One emoji fits comfortably; a sentence does not.
pub const MAX_EMOJI: usize = 16;

/// A label an item can carry — a shop, a category, a bit of workflow.
///
/// Tags are global reference data like units, not per-list: `Dairy` means the same
/// thing on every list, and the `name` uniqueness is what keeps it that way.
///
/// The `colour?:`/`emoji?:` annotations on every query below are load-bearing, not
/// noise. Adding the `CHECK` to those nullable columns flips sqlx's inference to
/// NOT NULL, and a `#[sqlx(transparent)]` newtype then decodes a NULL as
/// `Some(Colour(""))` rather than `None` — silently, with no error anywhere. The `?`
/// forces the nullable decode back on.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, PartialEq)]
pub struct Tag {
    pub id: Id,
    pub name: Name,
    pub colour: Option<Colour>,
    pub emoji: Option<Emoji>,
    pub created_at: CreatedAt,
    /// Where this tag falls when a list is grouped by category — the order of the
    /// shop, not the alphabet. Set by migration alongside the tag itself.
    pub sort_order: SortOrder,
}

/// How a caller asks for a single tag. Every variant must be able to identify at most
/// one row, which is why neither `Colour` nor `Emoji` is here — two tags may share a
/// colour, and `get`ting by one would quietly return whichever row sorted first.
/// `CreatedAt` is orderable but not a key either.
#[derive(Debug, Clone)]
pub enum Lookup {
    Id(Id),
    Name(Name),
}

/// What `list` may order by. Deliberately a separate enum from [`Lookup`] — the set
/// of sortable columns and the set of unique keys are not the same set.
///
/// Every variant added here needs a matching `WHEN` arm in both `CASE` branches of
/// the `list` query. A variant without one silently sorts by nothing, so
/// `list_every_field_changes_the_order` exists to fail the build's tests when the
/// two drift apart.
/// The default is `Name`: reference data is picked from, not browsed.
///
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray, serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Id,
    #[default]
    Name,
    Colour,
    Emoji,
    CreatedAt,
}

impl Tag {
    /// Inserts a tag under its [normalised](Name::normalized) name.
    ///
    /// A name that collides with an existing one is [`Error::Conflict`]; one that
    /// normalises to empty is [`Error::InvalidInput`]. `colour` and `emoji` are
    /// optional — a tag is legible without either — but a `Some` that trims to empty
    /// is [`Error::InvalidInput`] rather than a silent `None`.
    pub async fn create(
        pool: &sqlx::SqlitePool,
        name: Name,
        colour: Option<Colour>,
        emoji: Option<Emoji>,
    ) -> Result<Tag> {
        let name = name.normalized();
        let colour = colour.map(Colour::trimmed);
        let emoji = emoji.map(Emoji::trimmed);

        let tag = sqlx::query_as!(
            Tag,
            r#"
            INSERT INTO tags (name, colour, emoji)
            VALUES (?1, ?2, ?3)
            RETURNING
                id          as "id: Id",
                name        as "name: Name",
                colour      as "colour?: Colour",
                emoji       as "emoji?: Emoji",
                created_at  as "created_at!: CreatedAt",
                sort_order  as "sort_order: SortOrder"
            "#,
            name,
            colour,
            emoji,
        )
        .fetch_one(pool)
        .await?;

        Ok(tag)
    }

    /// Replaces a tag: name, colour, emoji.
    ///
    /// Every column is written each time, so `None` clears rather than keeps — there
    /// is no partial update. `RETURNING` with `fetch_one` means an id that matches no
    /// row is [`Error::NotFound`] without a second query.
    pub async fn update(
        pool: &sqlx::SqlitePool,
        id: Id,
        name: Name,
        colour: Option<Colour>,
        emoji: Option<Emoji>,
    ) -> Result<Tag> {
        let name = name.normalized();
        let colour = colour.map(Colour::trimmed);
        let emoji = emoji.map(Emoji::trimmed);

        let tag = sqlx::query_as!(
            Tag,
            r#"
            UPDATE tags SET name = ?1, colour = ?2, emoji = ?3 WHERE id = ?4
            RETURNING
                id          as "id: Id",
                name        as "name: Name",
                colour      as "colour?: Colour",
                emoji       as "emoji?: Emoji",
                created_at  as "created_at: CreatedAt",
                sort_order  as "sort_order: SortOrder"
            "#,
            name,
            colour,
            emoji,
            id,
        )
        .fetch_one(pool)
        .await?;

        Ok(tag)
    }

    /// Deletes a tag.
    ///
    /// `item_tags.tag_id` is `ON DELETE CASCADE`, not `RESTRICT`, so a tag still on
    /// items deletes and takes its links with it — untagging those items rather than
    /// refusing. That is deliberate: a tag is a label, and dropping a label is not
    /// blocked by the things wearing it. Nothing else references a tag, so this is
    /// never [`Error::InUse`].
    pub async fn delete(pool: &sqlx::SqlitePool, id: Id) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM tags WHERE id = ?1
            "#,
            id
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }

    /// Fetches one page of tags.
    ///
    /// Table-wide rather than scoped: tags are global reference data like units, so
    /// there is no owner to scope to. The per-item view is [`Tag::for_item`].
    ///
    /// The total is a second `count(*)` — see [`super::unit::Unit::list`] for why it
    /// is not folded into the page query as `count(*) OVER ()`.
    pub async fn list(
        pool: &sqlx::SqlitePool,
        page: Paging,
        order_by: OrderBy<Field>,
    ) -> Result<OffsetPage<Tag>> {
        let field: &'static str = order_by.field.into();
        let direction: &'static str = order_by.direction.into();

        let limit = page.limit();
        let offset = page.offset();

        let tags = sqlx::query_as!(
            Tag,
            r#"
        SELECT
            id          as "id: Id",
            name        as "name: Name",
            colour      as "colour?: Colour",
            emoji       as "emoji?: Emoji",
            created_at  as "created_at: CreatedAt",
            sort_order  as "sort_order: SortOrder"
        FROM tags
        ORDER BY
            CASE
                WHEN ?2 = 'ascending' THEN
                    CASE ?1
                        WHEN 'id' THEN id
                        -- NOCASE is a backstop only. Names are normalised in Rust
                        -- before they are written, and nothing in the schema enforces
                        -- that, so a row written by anything other than this model
                        -- (a fixture, a migration, a hand-run statement) may not be.
                        WHEN 'name' THEN name COLLATE NOCASE
                        WHEN 'colour' THEN colour
                        WHEN 'emoji' THEN emoji
                        WHEN 'created_at' THEN created_at
                    END
                END ASC NULLS LAST,
            CASE
                WHEN ?2 = 'descending' THEN
                    CASE ?1
                        WHEN 'id' THEN id
                        WHEN 'name' THEN name COLLATE NOCASE
                        WHEN 'colour' THEN colour
                        WHEN 'emoji' THEN emoji
                        WHEN 'created_at' THEN created_at
                    END
            END DESC NULLS LAST,
            -- keeps paging deterministic when the sort key ties
            id ASC
        LIMIT ?3 OFFSET ?4
        "#,
            field,
            direction,
            limit,
            offset,
        )
        .fetch_all(pool)
        .await?;

        let total = sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM tags"#)
            .fetch_one(pool)
            .await?;

        Ok(page.page_of(tags, total))
    }

    /// Fetches one tag.
    ///
    /// Names are [normalised](Name::normalized) before matching, so a lookup finds the row whatever
    /// case the caller had it in. A miss is [`Error::NotFound`], not `Ok(None)`.
    pub async fn get(pool: &sqlx::SqlitePool, by: Lookup) -> Result<Tag> {
        let tag = match by {
            Lookup::Id(v) => {
                sqlx::query_as!(
                    Tag,
                    r#"
                SELECT
                    id          as "id: Id",
                    name        as "name: Name",
                    colour      as "colour?: Colour",
                    emoji       as "emoji?: Emoji",
                    created_at  as "created_at: CreatedAt",
                sort_order  as "sort_order: SortOrder"
                FROM tags
                WHERE id = ?1 "#,
                    v
                )
                .fetch_one(pool)
                .await?
            }
            Lookup::Name(v) => {
                let name = v.normalized();
                sqlx::query_as!(
                    Tag,
                    r#"
                SELECT
                    id          as "id: Id",
                    name        as "name: Name",
                    colour      as "colour?: Colour",
                    emoji       as "emoji?: Emoji",
                    created_at  as "created_at: CreatedAt",
                sort_order  as "sort_order: SortOrder"
                FROM tags
                WHERE name = ?1 "#,
                    name
                )
                .fetch_one(pool)
                .await?
            }
        };
        Ok(tag)
    }

    /// Every tag on one item, by name.
    ///
    /// Not paged: an item carries a handful of tags, not a table's worth, and a caller
    /// rendering a line wants all of them or none. An item that does not exist and an
    /// item with no tags are both an empty `Vec` — this is a projection of the item,
    /// so it has no miss of its own to report.
    pub async fn for_item(pool: &sqlx::SqlitePool, item_id: item::Id) -> Result<Vec<Tag>> {
        let tags = sqlx::query_as!(
            Tag,
            r#"
            SELECT
                t.id          as "id: Id",
                t.name        as "name: Name",
                t.colour      as "colour?: Colour",
                t.emoji       as "emoji?: Emoji",
                t.created_at  as "created_at: CreatedAt",
                t.sort_order  as "sort_order: SortOrder"
            FROM tags t
            JOIN item_tags it ON it.tag_id = t.id
            WHERE it.item_id = ?1
            ORDER BY t.name COLLATE NOCASE, t.id
            "#,
            item_id,
        )
        .fetch_all(pool)
        .await?;

        Ok(tags)
    }

    /// Puts a tag on an item.
    ///
    /// `item_tags` is keyed on the pair, so attaching the same tag twice is
    /// [`Error::Conflict`] rather than a silent no-op — the caller asked for something
    /// that had already happened, and only they know whether that matters. An item or
    /// tag that does not exist is [`Error::InvalidInput`]: the row named a parent that
    /// is not there, which is the caller's mistake, not a delete held back by
    /// dependants ([`Error::InUse`]).
    /// Every tag on every item of one list, as `(item_id, tag)` pairs.
    ///
    /// One query rather than one per item: a list page needs the tags for everything
    /// on it, and asking per item turns a twenty-line list into twenty-one round
    /// trips. Ordered by item then by `sort_order`, so each item's first tag is the
    /// one that decides which group it falls into when the list is grouped.
    pub async fn for_list(
        pool: &sqlx::SqlitePool,
        list_id: list::Id,
    ) -> Result<Vec<(item::Id, Tag)>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                it.item_id  as "item_id: item::Id",
                t.id        as "id: Id",
                t.name      as "name: Name",
                t.colour    as "colour?: Colour",
                t.emoji     as "emoji?: Emoji",
                t.created_at as "created_at: CreatedAt",
                t.sort_order as "sort_order: SortOrder"
            FROM item_tags it
            JOIN tags t  ON t.id = it.tag_id
            JOIN items i ON i.id = it.item_id
            WHERE i.list_id = ?1
            ORDER BY it.item_id, t.sort_order, t.name
            "#,
            list_id
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.item_id,
                    Tag {
                        id: r.id,
                        name: r.name,
                        colour: r.colour,
                        emoji: r.emoji,
                        created_at: r.created_at,
                        sort_order: r.sort_order,
                    },
                )
            })
            .collect())
    }

    pub async fn attach(pool: &sqlx::SqlitePool, item_id: item::Id, tag_id: Id) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO item_tags (item_id, tag_id) VALUES (?1, ?2)
            "#,
            item_id,
            tag_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Takes a tag off an item.
    ///
    /// A pair that was not attached is [`Error::NotFound`], the mirror of `attach`
    /// reporting [`Error::Conflict`] for one that already was. Neither the item nor
    /// the tag is touched — only the link between them.
    pub async fn detach(pool: &sqlx::SqlitePool, item_id: item::Id, tag_id: Id) -> Result<()> {
        let result = sqlx::query!(
            r#"
            DELETE FROM item_tags WHERE item_id = ?1 AND tag_id = ?2
            "#,
            item_id,
            tag_id,
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(Error::NotFound);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use sqlx::SqlitePool;
    use time::Duration;

    use strum::VariantArray;

    use super::*;
    use crate::models::{Direction, pool};

    /// Everything `tags.sql` needs under it — see `fixtures/README.md` for the order.
    macro_rules! fixtures {
        () => {
            seeds!(
                "fixtures/users.sql",
                "fixtures/lists.sql",
                "fixtures/units.sql",
                "fixtures/items.sql",
                "fixtures/tags.sql",
            )
        };
    }

    /// Tags in `fixtures/tags.sql`.
    const SEEDED: i64 = 21;
    /// Rows in `item_tags`, and the items carrying none of them.
    const SEEDED_LINKS: i64 = 150;
    const UNTAGGED: &str = "Cake candles";
    /// A tag the fixture puts on plenty of items, so deleting it has links to cascade.
    const BUSY: &str = "tesco";

    fn all_tags() -> Paging {
        Paging {
            number: 1,
            size: 100,
        }
    }

    fn by(field: Field, direction: Direction) -> OrderBy<Field> {
        OrderBy { field, direction }
    }

    async fn any_tag(pool: &SqlitePool) -> Result<Tag> {
        let mut page = Tag::list(pool, all_tags(), by(Field::Id, Direction::Ascending)).await?;
        Ok(page.items.swap_remove(0))
    }

    async fn item_id(pool: &SqlitePool, name: &str) -> Result<item::Id> {
        Ok(sqlx::query_scalar!(
            r#"SELECT id as "id!: item::Id" FROM items WHERE name = ?1"#,
            name
        )
        .fetch_one(pool)
        .await?)
    }

    async fn count(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM tags"#)
                .fetch_one(pool)
                .await?,
        )
    }

    async fn links(pool: &SqlitePool) -> Result<i64> {
        Ok(
            sqlx::query_scalar!(r#"SELECT count(*) as "total!: i64" FROM item_tags"#)
                .fetch_one(pool)
                .await?,
        )
    }

    fn ids(p: &OffsetPage<Tag>) -> Vec<Id> {
        p.items.iter().map(|t| t.id).collect()
    }

    fn names(p: &OffsetPage<Tag>) -> Vec<Name> {
        p.items.iter().map(|t| t.name.clone()).collect()
    }

    fn colours(p: &OffsetPage<Tag>) -> Vec<Option<Colour>> {
        p.items.iter().map(|t| t.colour.clone()).collect()
    }

    fn emojis(p: &OffsetPage<Tag>) -> Vec<Option<Emoji>> {
        p.items.iter().map(|t| t.emoji.clone()).collect()
    }

    fn created_ats(p: &OffsetPage<Tag>) -> Vec<CreatedAt> {
        p.items.iter().map(|t| t.created_at).collect()
    }

    /// Every `Some` sorted in `direction`, and every `None` after all of them.
    fn sorted_nulls_last<T: PartialOrd>(vals: &[Option<T>], direction: Direction) -> bool {
        let first_null = vals.iter().position(Option::is_none).unwrap_or(vals.len());
        if vals[first_null..].iter().any(Option::is_some) {
            return false;
        }
        let present = &vals[..first_null];

        match direction {
            Direction::Ascending => present.windows(2).all(|w| w[0] <= w[1]),
            Direction::Descending => present.windows(2).all(|w| w[0] >= w[1]),
        }
    }

    // ---------------------------------------------------------------- create

    #[rstest]
    #[case::plain("produce", Ok("produce"))]
    #[case::multi_word("meat & fish", Ok("meat & fish"))]
    #[case::trims_whitespace("    produce  ", Ok("produce"))]
    #[case::lowercases("Meat & Fish", Ok("meat & fish"))]
    // SQLite's lower() folds ASCII only, so this is stored unchanged if the model
    // normalises in SQL rather than in Rust
    #[case::lowercases_non_ascii("Ångström", Ok("ångström"))]
    #[case::rejects_empty("", Err(Error::InvalidInput))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    // str::trim strips every Unicode space; SQLite's trim() only strips U+0020
    #[case::rejects_non_breaking_space_only("\u{00A0}", Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create(
        #[future(awt)] pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let got = Tag::create(
            &pool,
            Name(input.into()),
            Some(Colour("#00539F".into())),
            Some(Emoji("🛒".into())),
        )
        .await;

        match (got, expected) {
            (Ok(tag), Ok(want)) => {
                assert_eq!(tag.name, Name(want.into()), "stored under its name");
                assert_eq!(
                    tag.colour,
                    Some(Colour("#00539F".into())),
                    "a colour keeps its case"
                );
                assert_eq!(tag.emoji, Some(Emoji("🛒".into())));
                let age = OffsetDateTime::now_utc() - tag.created_at.0;
                assert!(
                    (Duration::ZERO..Duration::minutes(1)).contains(&age),
                    "created_at should be stamped now, was {age} ago"
                );
                assert_eq!(
                    Tag::get(&pool, Lookup::Id(tag.id)).await?,
                    tag,
                    "the returned row is the one that was written"
                );
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(count(&pool).await?, 0, "a rejected name must not insert");
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    /// `colour` and `emoji` are nullable. A `None` must survive the round trip as
    /// `None` rather than as `Some("")` — see the note on [`Tag`] about the `?:`
    /// annotations, which is the only thing keeping this true.
    #[rstest]
    #[case::no_colour(None, Some("🛒"))]
    #[case::no_emoji(Some("#00539F"), None)]
    #[case::neither(None, None)]
    #[tokio::test]
    async fn create_without_a_colour_or_emoji(
        #[future(awt)] pool: SqlitePool,
        #[case] colour: Option<&str>,
        #[case] emoji: Option<&str>,
    ) -> Result<()> {
        let want_colour = colour.map(|c| Colour(c.into()));
        let want_emoji = emoji.map(|e| Emoji(e.into()));

        let tag = Tag::create(
            &pool,
            Name("sparse".into()),
            want_colour.clone(),
            want_emoji.clone(),
        )
        .await?;

        assert_eq!(tag.colour, want_colour);
        assert_eq!(tag.emoji, want_emoji);

        let read_back = Tag::get(&pool, Lookup::Id(tag.id)).await?;
        assert_eq!(read_back.colour, want_colour, "NULL decoded as Some(\"\")");
        assert_eq!(read_back.emoji, want_emoji, "NULL decoded as Some(\"\")");
        assert_eq!(read_back, tag);
        Ok(())
    }

    /// A `Some` that trims to nothing is not a `None`: the caller passed a value, and
    /// an empty one is not storable. The `CHECK` says so rather than the column
    /// quietly holding `''`.
    #[rstest]
    #[case::empty_colour(Some(""), Some("🛒"))]
    #[case::whitespace_colour(Some("   "), Some("🛒"))]
    #[case::empty_emoji(Some("#00539F"), Some(""))]
    #[case::whitespace_emoji(Some("#00539F"), Some("   "))]
    #[tokio::test]
    async fn create_rejects_an_empty_colour_or_emoji(
        #[future(awt)] pool: SqlitePool,
        #[case] colour: Option<&str>,
        #[case] emoji: Option<&str>,
    ) -> Result<()> {
        let result = Tag::create(
            &pool,
            Name("sparse".into()),
            colour.map(|c| Colour(c.into())),
            emoji.map(|e| Emoji(e.into())),
        )
        .await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(count(&pool).await?, 0);
        Ok(())
    }

    /// Padding comes off a colour and an emoji, but the case does not — `#00539F` is
    /// hex a person reads back, and folding an emoji is meaningless.
    #[rstest]
    #[tokio::test]
    async fn create_trims_the_colour_and_emoji(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let tag = Tag::create(
            &pool,
            Name("tesco".into()),
            Some(Colour("  #00539F ".into())),
            Some(Emoji(" 🛠️ ".into())),
        )
        .await?;

        assert_eq!(
            tag.colour,
            Some(Colour("#00539F".into())),
            "trimmed, not folded"
        );
        assert_eq!(tag.emoji, Some(Emoji("🛠️".into())));
        assert_eq!(Tag::get(&pool, Lookup::Id(tag.id)).await?, tag);
        Ok(())
    }

    /// Names are bounded, so a caller cannot park a megabyte in the table.
    #[rstest]
    #[case::at_the_limit(MAX_NAME, Ok(()))]
    #[case::one_over_the_limit(MAX_NAME + 1, Err(Error::InvalidInput))]
    #[case::absurd(100_000, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_the_name_length(
        #[future(awt)] pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let name = Name("x".repeat(length));

        let got = Tag::create(&pool, name, None, None).await.map(|_| ());

        assert_eq!(got, expected, "a name of {length} characters");
        Ok(())
    }

    /// The presentation columns are bounded too — an emoji is a character or two, not
    /// a paragraph.
    #[rstest]
    #[case::colour_at_the_limit(MAX_COLOUR, MAX_EMOJI, Ok(()))]
    #[case::colour_over_the_limit(MAX_COLOUR + 1, MAX_EMOJI, Err(Error::InvalidInput))]
    #[case::emoji_over_the_limit(MAX_COLOUR, MAX_EMOJI + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn create_bounds_the_colour_and_emoji_length(
        #[future(awt)] pool: SqlitePool,
        #[case] colour: usize,
        #[case] emoji: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let got = Tag::create(
            &pool,
            Name("bounded".into()),
            Some(Colour("c".repeat(colour))),
            Some(Emoji("e".repeat(emoji))),
        )
        .await
        .map(|_| ());

        assert_eq!(got, expected, "colour of {colour}, emoji of {emoji}");
        Ok(())
    }

    /// The bound applies to renames too, not just inserts.
    #[rstest]
    #[case::at_the_limit(MAX_NAME, Ok(()))]
    #[case::one_over_the_limit(MAX_NAME + 1, Err(Error::InvalidInput))]
    #[tokio::test]
    async fn update_bounds_the_name_length(
        #[future(awt)] pool: SqlitePool,
        #[case] length: usize,
        #[case] expected: Result<()>,
    ) -> Result<()> {
        let tag = Tag::create(&pool, Name("my-tag".into()), None, None).await?;

        let got = Tag::update(&pool, tag.id, Name("x".repeat(length)), None, None)
            .await
            .map(|_| ());

        assert_eq!(got, expected, "a name of {length} characters");
        Ok(())
    }

    /// `tags.name` is `UNIQUE`, and every name is normalised before it is stored, so
    /// a second tag collides whenever it differs only in case or padding.
    #[rstest]
    #[case::exact_duplicate("produce")]
    #[case::differs_only_in_case("ProDuce")]
    #[case::differs_only_in_padding("  produce ")]
    #[tokio::test]
    async fn create_rejects_duplicate_name(
        #[future(awt)] pool: SqlitePool,
        #[case] duplicate: &str,
    ) -> Result<()> {
        Tag::create(&pool, Name("produce".into()), None, None).await?;

        let err = Tag::create(&pool, Name(duplicate.into()), None, None)
            .await
            .expect_err("duplicate name must not insert");

        assert_eq!(err, Error::Conflict);
        assert_eq!(
            count(&pool).await?,
            1,
            "the failed insert must not add a row"
        );
        Ok(())
    }

    /// The non-ASCII half of the same rule. `COLLATE NOCASE` does not fold `Å`, so
    /// this only holds because the name is lowercased in Rust before it is stored.
    #[rstest]
    #[tokio::test]
    async fn create_rejects_duplicate_name_non_ascii(
        #[future(awt)] pool: SqlitePool,
    ) -> Result<()> {
        Tag::create(&pool, Name("ångström".into()), None, None).await?;

        let err = Tag::create(&pool, Name("Ångström".into()), None, None)
            .await
            .expect_err("duplicate name must not insert");

        assert_eq!(err, Error::Conflict);
        assert_eq!(
            count(&pool).await?,
            1,
            "the failed insert must not add a row"
        );
        Ok(())
    }

    // ---------------------------------------------------------------- update

    #[rstest]
    #[case::renames("my-renamed-tag", Ok("my-renamed-tag"))]
    #[case::to_the_same_name("my-tag", Ok("my-tag"))]
    #[case::trims_whitespace(" my-renamed-tag     ", Ok("my-renamed-tag"))]
    #[case::lowercases(" My-Renamed-Tag     ", Ok("my-renamed-tag"))]
    #[case::lowercases_non_ascii("Ångström", Ok("ångström"))]
    #[case::rejects_whitespace_only("   ", Err(Error::InvalidInput))]
    #[case::rejects_a_name_already_taken("produce", Err(Error::Conflict))]
    #[case::rejects_a_taken_name_in_another_case("ProDuce", Err(Error::Conflict))]
    #[tokio::test]
    async fn update(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
        #[case] expected: Result<&str>,
    ) -> Result<()> {
        let tag = Tag::create(&pool, Name("my-tag".into()), None, None).await?;

        let result = Tag::update(
            &pool,
            tag.id,
            Name(input.into()),
            Some(Colour("  #FFD700 ".into())),
            Some(Emoji(" ⭐ ".into())),
        )
        .await;

        match (result, expected) {
            (Ok(renamed), Ok(want)) => {
                assert_eq!(renamed.id, tag.id, "an edit must not move the row");
                assert_eq!(renamed.name, Name(want.into()));
                assert_eq!(
                    renamed.colour,
                    Some(Colour("#FFD700".into())),
                    "trimmed, case kept"
                );
                assert_eq!(renamed.emoji, Some(Emoji("⭐".into())));
                assert_eq!(
                    renamed.created_at, tag.created_at,
                    "an edit must not restamp created_at"
                );
                assert_eq!(Tag::get(&pool, Lookup::Id(tag.id)).await?, renamed);
            }
            (Err(got), Err(want)) => {
                assert_eq!(got, want);
                assert_eq!(
                    Tag::get(&pool, Lookup::Id(tag.id)).await?,
                    tag,
                    "a rejected edit must leave the row alone"
                );
            }
            (got, expected) => panic!("expected {expected:?}, got {got:?}"),
        }
        Ok(())
    }

    /// Every column is written each time, so a `None` clears what was there. There is
    /// no partial update, and this is what says so.
    #[rstest]
    #[tokio::test]
    async fn update_clears_with_none(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let before = Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?;
        assert!(
            before.colour.is_some() && before.emoji.is_some(),
            "need both set to prove they clear"
        );

        let after = Tag::update(&pool, before.id, before.name.clone(), None, None).await?;

        assert_eq!(after.colour, None);
        assert_eq!(after.emoji, None);
        assert_eq!(after.name, before.name, "clearing is not a rename");
        assert_eq!(Tag::get(&pool, Lookup::Id(before.id)).await?, after);
        Ok(())
    }

    #[rstest]
    #[case::rejects_empty_colour(Some(""), None)]
    #[case::rejects_whitespace_emoji(None, Some("   "))]
    #[tokio::test]
    async fn update_rejects_bad_input(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] colour: Option<&str>,
        #[case] emoji: Option<&str>,
    ) -> Result<()> {
        let before = Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?;

        let result = Tag::update(
            &pool,
            before.id,
            before.name.clone(),
            colour.map(|c| Colour(c.into())),
            emoji.map(|e| Emoji(e.into())),
        )
        .await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(
            Tag::get(&pool, Lookup::Id(before.id)).await?,
            before,
            "a rejected edit must leave the row alone"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn update_reports_a_miss(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let result = Tag::update(&pool, Id(9999), Name("nothing".into()), None, None).await;

        assert!(
            matches!(result, Err(Error::NotFound)),
            "expected NotFound, got {result:?}"
        );
        assert_eq!(
            count(&pool).await?,
            SEEDED,
            "a missed update must not insert"
        );
        Ok(())
    }

    // ---------------------------------------------------------------- delete

    #[rstest]
    #[case::a_shop("boots")]
    #[case::a_category("fruits")]
    #[tokio::test]
    async fn delete(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] name: &str,
    ) -> Result<()> {
        let tag = Tag::get(&pool, Lookup::Name(Name(name.into()))).await?;

        Tag::delete(&pool, tag.id).await?;

        assert!(
            matches!(
                Tag::get(&pool, Lookup::Id(tag.id)).await,
                Err(Error::NotFound)
            ),
            "the row is gone"
        );
        assert_eq!(count(&pool).await?, SEEDED - 1);

        let result = Tag::delete(&pool, tag.id).await;
        assert!(
            matches!(result, Err(Error::NotFound)),
            "deleting it twice reports the miss, got {result:?}"
        );
        Ok(())
    }

    /// `item_tags.tag_id` is `ON DELETE CASCADE`, not `ON DELETE RESTRICT` as
    /// `items.unit_id` is — so unlike `unit::Unit::delete`, this is *not*
    /// [`Error::InUse`]. The tag goes, the items stay, and only the links between them
    /// are removed.
    #[rstest]
    #[tokio::test]
    async fn delete_cascades_to_the_items_wearing_it(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let tag = Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?;
        let attached = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM item_tags WHERE tag_id = ?1"#,
            tag.id
        )
        .fetch_one(&pool)
        .await?;
        assert!(attached > 0, "need a tag in use to prove the cascade");
        let items_before = sqlx::query_scalar!(r#"SELECT count(*) as "n!: i64" FROM items"#)
            .fetch_one(&pool)
            .await?;

        Tag::delete(&pool, tag.id).await?;

        assert_eq!(
            links(&pool).await?,
            SEEDED_LINKS - attached,
            "its {attached} links went with it"
        );
        assert_eq!(
            sqlx::query_scalar!(r#"SELECT count(*) as "n!: i64" FROM items"#)
                .fetch_one(&pool)
                .await?,
            items_before,
            "the items it was on must survive it"
        );
        Ok(())
    }

    /// One query for a whole list's tags, so a page does not make one round trip per
    /// item. The pairs must cover every tagged item on the list and nothing else.
    #[rstest]
    #[tokio::test]
    async fn for_list_covers_the_whole_list(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
            "fixtures/tags.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let list_id = sqlx::query_scalar!(
            r#"SELECT id as "id!: list::Id" FROM lists WHERE name = 'Fruit & veg'"#
        )
        .fetch_one(&pool)
        .await?;

        let pairs = Tag::for_list(&pool, list_id).await?;

        // every pair belongs to an item on this list
        let on_list = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM items WHERE list_id = ?1"#,
            list_id
        )
        .fetch_one(&pool)
        .await?;
        let items: std::collections::HashSet<_> = pairs.iter().map(|(i, _)| i.0).collect();
        assert!(!items.is_empty(), "the fixture list has tagged items");
        assert!(items.len() <= on_list as usize);

        // and it agrees with asking item by item
        for item_id in &items {
            let one = Tag::for_item(&pool, item::Id(*item_id)).await?;
            let batched: Vec<_> = pairs
                .iter()
                .filter(|(i, _)| i.0 == *item_id)
                .map(|(_, t)| t.clone())
                .collect();
            assert_eq!(batched, one, "batched and per-item disagree for {item_id}");
        }

        // the fixture leaves one item on this list untagged, and it must not appear
        let untagged = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM items
               WHERE list_id = ?1 AND id NOT IN (SELECT item_id FROM item_tags)"#,
            list_id
        )
        .fetch_one(&pool)
        .await?;
        assert!(
            untagged > 0,
            "the fixture should have an untagged item here"
        );
        assert_eq!(items.len(), (on_list - untagged) as usize);

        Ok(())
    }

    /// Another list's tags must not leak in.
    #[rstest]
    #[tokio::test]
    async fn for_list_is_scoped(
        #[with(seeds!(
            "fixtures/users.sql",
            "fixtures/lists.sql",
            "fixtures/units.sql",
            "fixtures/items.sql",
            "fixtures/tags.sql",
        ))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let a = sqlx::query_scalar!(
            r#"SELECT id as "id!: list::Id" FROM lists WHERE name = 'Fruit & veg'"#
        )
        .fetch_one(&pool)
        .await?;
        let b =
            sqlx::query_scalar!(r#"SELECT id as "id!: list::Id" FROM lists WHERE name = 'Dairy'"#)
                .fetch_one(&pool)
                .await?;

        let from_a: std::collections::HashSet<_> = Tag::for_list(&pool, a)
            .await?
            .into_iter()
            .map(|(i, _)| i.0)
            .collect();
        let from_b: std::collections::HashSet<_> = Tag::for_list(&pool, b)
            .await?
            .into_iter()
            .map(|(i, _)| i.0)
            .collect();

        assert!(!from_a.is_empty() && !from_b.is_empty());
        assert!(
            from_a.is_disjoint(&from_b),
            "one list's items appeared in another's"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn for_list_of_nothing_is_empty(
        #[with(seeds!("fixtures/users.sql", "fixtures/lists.sql"))]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let empty = sqlx::query_scalar!(
            r#"SELECT id as "id!: list::Id" FROM lists WHERE name = 'Chemist'"#
        )
        .fetch_one(&pool)
        .await?;

        assert!(Tag::for_list(&pool, empty).await?.is_empty());
        Ok(())
    }

    // ---------------------------------------------------------------- lookup

    #[rstest]
    #[tokio::test]
    async fn get(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let want = any_tag(&pool).await?;

        assert_eq!(Tag::get(&pool, Lookup::Id(want.id)).await?, want);
        assert_eq!(
            Tag::get(&pool, Lookup::Name(want.name.clone())).await?,
            want
        );
        Ok(())
    }

    /// Callers do not have to know the stored form of a name to look one up.
    #[rstest]
    #[case::shouted("TESCO")]
    #[case::mixed_case("Tesco")]
    #[case::padded("  tesco  ")]
    #[tokio::test]
    async fn get_by_name_normalises_the_lookup(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] input: &str,
    ) -> Result<()> {
        let want = Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?;

        assert_eq!(
            Tag::get(&pool, Lookup::Name(Name(input.into()))).await?,
            want
        );
        Ok(())
    }

    /// The non-ASCII half: `COLLATE NOCASE` cannot fold `Å`, so this passes only
    /// because the lookup is normalised in Rust.
    #[rstest]
    #[tokio::test]
    async fn get_by_name_normalises_non_ascii(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let want = Tag::create(&pool, Name("ångström".into()), None, None).await?;

        assert_eq!(
            Tag::get(&pool, Lookup::Name(Name("Ångström".into()))).await?,
            want
        );
        Ok(())
    }

    /// `fetch_one` means a miss is an error, not `Ok(None)`. Every caller has to
    /// handle that, so it is worth stating.
    #[rstest]
    #[case::missing_id(Lookup::Id(Id(9999)))]
    #[case::zero_id(Lookup::Id(Id(0)))]
    #[case::unknown_name(Lookup::Name(Name("nonesuch".into())))]
    #[case::empty_name(Lookup::Name(Name("".into())))]
    #[tokio::test]
    async fn get_reports_a_miss(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] lookup: Lookup,
    ) {
        assert!(matches!(
            Tag::get(&pool, lookup).await,
            Err(Error::NotFound)
        ));
    }

    // --------------------------------------------------------------- tagging

    #[rstest]
    #[tokio::test]
    async fn for_item_returns_only_that_items_tags(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let apples = item_id(&pool, "Apples").await?;

        let tags = Tag::for_item(&pool, apples).await?;

        assert!(!tags.is_empty(), "Apples is tagged in the fixture");
        assert!(
            tags.len() < SEEDED as usize,
            "every tag came back, so nothing is scoping"
        );
        let mut sorted = tags.clone();
        sorted.sort_by(|a, b| a.name.0.cmp(&b.name.0));
        assert_eq!(sorted, tags, "ordered by name");

        let linked = sqlx::query_scalar!(
            r#"SELECT count(*) as "n!: i64" FROM item_tags WHERE item_id = ?1"#,
            apples
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(tags.len() as i64, linked);
        Ok(())
    }

    /// An item with no tags and an item that does not exist both come back empty:
    /// this is a projection of an item, so it has no miss of its own to report.
    #[rstest]
    #[tokio::test]
    async fn for_item_is_empty_without_tags(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let untagged = item_id(&pool, UNTAGGED).await?;

        assert!(Tag::for_item(&pool, untagged).await?.is_empty());
        assert!(Tag::for_item(&pool, item::Id(9999)).await?.is_empty());
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn attach_and_detach(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let untagged = item_id(&pool, UNTAGGED).await?;
        let tag = Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?;

        Tag::attach(&pool, untagged, tag.id).await?;

        assert_eq!(Tag::for_item(&pool, untagged).await?, vec![tag.clone()]);
        assert_eq!(links(&pool).await?, SEEDED_LINKS + 1);

        Tag::detach(&pool, untagged, tag.id).await?;

        assert!(Tag::for_item(&pool, untagged).await?.is_empty());
        assert_eq!(links(&pool).await?, SEEDED_LINKS);
        assert_eq!(
            count(&pool).await?,
            SEEDED,
            "detaching must not delete the tag"
        );
        Ok(())
    }

    /// `item_tags` is keyed on the pair, so a second attach is a unique violation
    /// rather than a no-op.
    #[rstest]
    #[tokio::test]
    async fn attach_twice_reports_a_conflict(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let untagged = item_id(&pool, UNTAGGED).await?;
        let tag = Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?;
        Tag::attach(&pool, untagged, tag.id).await?;

        let result = Tag::attach(&pool, untagged, tag.id).await;

        assert!(
            matches!(result, Err(Error::Conflict)),
            "expected Conflict, got {result:?}"
        );
        assert_eq!(links(&pool).await?, SEEDED_LINKS + 1, "no second link");
        Ok(())
    }

    /// Naming a parent that is not there is the caller's mistake, and it is
    /// [`Error::InvalidInput`] — not [`Error::InUse`], which is a *delete* held back
    /// by dependants.
    #[rstest]
    #[case::unknown_item(true, false)]
    #[case::unknown_tag(false, true)]
    #[case::neither_exists(true, true)]
    #[tokio::test]
    async fn attach_rejects_a_dangling_reference(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] bad_item: bool,
        #[case] bad_tag: bool,
    ) -> Result<()> {
        let item = if bad_item {
            item::Id(9999)
        } else {
            item_id(&pool, UNTAGGED).await?
        };
        let tag = if bad_tag {
            Id(9999)
        } else {
            Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?.id
        };

        let result = Tag::attach(&pool, item, tag).await;

        assert!(
            matches!(result, Err(Error::InvalidInput)),
            "expected InvalidInput, got {result:?}"
        );
        assert_eq!(links(&pool).await?, SEEDED_LINKS, "nothing was linked");
        Ok(())
    }

    /// The mirror of `attach` reporting a conflict for a link that already exists.
    #[rstest]
    #[case::never_attached(false, false)]
    #[case::unknown_item(true, false)]
    #[case::unknown_tag(false, true)]
    #[tokio::test]
    async fn detach_reports_a_miss(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] bad_item: bool,
        #[case] bad_tag: bool,
    ) -> Result<()> {
        let item = if bad_item {
            item::Id(9999)
        } else {
            item_id(&pool, UNTAGGED).await?
        };
        let tag = if bad_tag {
            Id(9999)
        } else {
            Tag::get(&pool, Lookup::Name(Name(BUSY.into()))).await?.id
        };

        let result = Tag::detach(&pool, item, tag).await;

        assert!(
            matches!(result, Err(Error::NotFound)),
            "expected NotFound, got {result:?}"
        );
        assert_eq!(links(&pool).await?, SEEDED_LINKS, "nothing was unlinked");
        Ok(())
    }

    // -------------------------------------------------------------- ordering

    struct OrderCase {
        order_by: OrderBy<Field>,
        assert: fn(&OffsetPage<Tag>),
    }

    #[rstest]
    #[case::id_ascending(OrderCase {
        order_by: OrderBy { field: Field::Id, direction: Direction::Ascending },
        assert: |p| assert!(ids(p).windows(2).all(|w| w[0].0 < w[1].0), "{:?}", ids(p)),
    })]
    #[case::id_descending(OrderCase {
        order_by: OrderBy { field: Field::Id, direction: Direction::Descending },
        assert: |p| assert!(ids(p).windows(2).all(|w| w[0].0 > w[1].0), "{:?}", ids(p)),
    })]
    #[case::name_ascending(OrderCase {
        order_by: OrderBy { field: Field::Name, direction: Direction::Ascending },
        assert: |p| assert!(names(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", names(p)),
    })]
    #[case::name_descending(OrderCase {
        order_by: OrderBy { field: Field::Name, direction: Direction::Descending },
        assert: |p| assert!(names(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", names(p)),
    })]
    #[case::colour_ascending(OrderCase {
        order_by: OrderBy { field: Field::Colour, direction: Direction::Ascending },
        assert: |p| assert!(sorted_nulls_last(&colours(p), Direction::Ascending), "{:?}", colours(p)),
    })]
    #[case::colour_descending(OrderCase {
        order_by: OrderBy { field: Field::Colour, direction: Direction::Descending },
        assert: |p| assert!(sorted_nulls_last(&colours(p), Direction::Descending), "{:?}", colours(p)),
    })]
    #[case::emoji_ascending(OrderCase {
        order_by: OrderBy { field: Field::Emoji, direction: Direction::Ascending },
        assert: |p| assert!(sorted_nulls_last(&emojis(p), Direction::Ascending), "{:?}", emojis(p)),
    })]
    #[case::emoji_descending(OrderCase {
        order_by: OrderBy { field: Field::Emoji, direction: Direction::Descending },
        assert: |p| assert!(sorted_nulls_last(&emojis(p), Direction::Descending), "{:?}", emojis(p)),
    })]
    #[case::created_at_ascending(OrderCase {
        order_by: OrderBy { field: Field::CreatedAt, direction: Direction::Ascending },
        assert: |p| assert!(created_ats(p).windows(2).all(|w| w[0].0 <= w[1].0), "{:?}", created_ats(p)),
    })]
    #[case::created_at_descending(OrderCase {
        order_by: OrderBy { field: Field::CreatedAt, direction: Direction::Descending },
        assert: |p| assert!(created_ats(p).windows(2).all(|w| w[0].0 >= w[1].0), "{:?}", created_ats(p)),
    })]
    #[tokio::test]
    async fn list_orders_by_every_field(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: OrderCase,
    ) -> Result<()> {
        let page = Tag::list(&pool, all_tags(), c.order_by).await?;
        assert_eq!(page.items.len(), SEEDED as usize);
        (c.assert)(&page);
        Ok(())
    }

    /// Each field must produce a *different* order. A [`Field`] variant with no
    /// matching arm in the SQL `CASE` falls through to NULL for every row, which
    /// orders nothing and raises no error — this is what catches that.
    ///
    /// It iterates `Field::VARIANTS` rather than a hand-written list on purpose: an
    /// array is not exhaustiveness-checked, so a variant added later would slip past
    /// it silently. It also works only because `fixtures/tags.sql` stamps `created_at`
    /// deliberately out of id order; with the column default every row would share a
    /// timestamp and this would be a false negative.
    #[rstest]
    #[tokio::test]
    async fn list_every_field_changes_the_order(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        let mut orders = Vec::new();
        for &field in Field::VARIANTS {
            for direction in [Direction::Ascending, Direction::Descending] {
                let page = Tag::list(&pool, all_tags(), by(field, direction)).await?;
                orders.push((format!("{field:?} {direction:?}"), ids(&page)));
            }
        }

        for (i, (left_name, left)) in orders.iter().enumerate() {
            for (right_name, right) in orders.iter().skip(i + 1) {
                assert_ne!(
                    left, right,
                    "{left_name} and {right_name} returned the same order, so at least \
                     one of them is not ordering at all"
                );
            }
        }
        Ok(())
    }

    // --------------------------------------------------------------- paging

    struct PageCase {
        page: Paging,
        tags: usize,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_page(
        PageCase { page: Paging { number: 1, size: 6 }, tags: 6, total_pages: 4, has_more: true }
    )]
    #[case::middle_page(
        PageCase { page: Paging { number: 2, size: 6 }, tags: 6, total_pages: 4, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { page: Paging { number: 4, size: 6 }, tags: 3, total_pages: 4, has_more: false }
    )]
    #[case::page_larger_than_the_table(
        PageCase { page: Paging { number: 1, size: 100 }, tags: SEEDED as usize, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { page: Paging { number: 99, size: 6 }, tags: 0, total_pages: 4, has_more: false }
    )]
    // a negative LIMIT means "no limit" to SQLite; Paging::limit clamps it so that a
    // bad page size cannot dump the whole table
    #[case::negative_size_is_empty(
        PageCase { page: Paging { number: 1, size: -1 }, tags: 0, total_pages: 0, has_more: true }
    )]
    // offset would overflow i64 and panic in debug without the saturating multiply
    #[case::huge_page_number(
        PageCase { page: Paging { number: i64::MAX, size: 6 }, tags: 0, total_pages: 4, has_more: false }
    )]
    #[tokio::test]
    async fn list_pages(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] c: PageCase,
    ) -> Result<()> {
        let page = Tag::list(&pool, c.page, by(Field::Id, Direction::Ascending)).await?;

        assert_eq!(page.items.len(), c.tags, "tags on the page");
        assert_eq!(page.total, SEEDED, "total is independent of the page");
        assert_eq!(page.total_pages, c.total_pages);
        assert_eq!(page.has_more, c.has_more);
        Ok(())
    }

    /// Walking `has_more` must reach every tag exactly once. A swapped `LIMIT`/
    /// `OFFSET` binding still returns plausible pages, but not this.
    #[rstest]
    #[case::by_id(Field::Id)]
    #[case::by_name(Field::Name)]
    #[case::by_colour(Field::Colour)]
    #[case::by_emoji(Field::Emoji)]
    #[case::by_created_at(Field::CreatedAt)]
    #[tokio::test]
    async fn list_walks_every_tag_exactly_once(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
        #[case] field: Field,
    ) -> Result<()> {
        let mut seen = Vec::new();
        let mut number = 1;

        loop {
            let page = Tag::list(
                &pool,
                Paging { number, size: 6 },
                by(field, Direction::Ascending),
            )
            .await?;
            seen.extend(ids(&page));
            if !page.has_more {
                assert_eq!(
                    page.total_pages, number,
                    "has_more cleared on the last page"
                );
                break;
            }
            number += 1;
            assert!(number < 100, "has_more never cleared");
        }

        let mut unique = seen.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(seen.len(), SEEDED as usize, "paged over {seen:?}");
        assert_eq!(unique.len(), SEEDED as usize, "repeated a tag");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_totals_track_inserts(
        #[with(fixtures!())]
        #[future(awt)]
        pool: SqlitePool,
    ) -> Result<()> {
        // a size whose last page is partial, so one insert fills it rather than
        // spilling onto a new one
        const SIZE: i64 = 8;
        let pages = (SEEDED + SIZE - 1) / SIZE;
        let on_last = SEEDED - (pages - 1) * SIZE;
        assert!(on_last < SIZE, "SEEDED must not divide evenly into SIZE");

        let page = |n| Paging {
            number: n,
            size: SIZE,
        };
        let order = by(Field::Id, Direction::Ascending);

        let before = Tag::list(&pool, page(pages), order).await?;
        assert_eq!(before.total, SEEDED);
        assert_eq!(before.total_pages, pages);
        assert_eq!(before.items.len(), on_last as usize);
        assert!(!before.has_more);

        Tag::create(&pool, Name("my-tag".into()), None, None).await?;

        let after = Tag::list(&pool, page(pages), order).await?;
        assert_eq!(after.total, SEEDED + 1);
        assert_eq!(after.total_pages, pages, "the new tag fits the last page");
        assert_eq!(after.items.len(), on_last as usize + 1);
        assert!(!after.has_more);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn list_on_an_empty_table(#[future(awt)] pool: SqlitePool) -> Result<()> {
        let page = Tag::list(
            &pool,
            Paging {
                number: 1,
                size: 10,
            },
            by(Field::Name, Direction::Ascending),
        )
        .await?;

        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_more);
        Ok(())
    }
}
