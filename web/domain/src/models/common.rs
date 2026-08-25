#[derive(Debug, Clone, Copy)]
pub struct OrderBy<T> {
    pub field: T,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, serde::Deserialize)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Ascending,
    Descending,
}

/// A requested page: `number` is 1-based, `size` rows to a page.
///
/// The arithmetic is shared because getting it wrong is a data leak rather than a
/// cosmetic bug: SQLite reads a negative `LIMIT` as "no limit", so an unclamped page
/// size returns the whole table.
#[derive(Debug, Clone, Copy)]
pub struct Paging {
    pub number: i64,
    pub size: i64,
}

impl Paging {
    /// Rows to fetch, clamped at zero so a negative size cannot become "no limit".
    pub fn limit(&self) -> i64 {
        self.size.max(0)
    }

    /// Rows to skip. Page numbers below one are treated as the first page, and the
    /// multiplication saturates so that a huge page number cannot overflow.
    pub fn offset(&self) -> i64 {
        self.number
            .saturating_sub(1)
            .max(0)
            .saturating_mul(self.limit())
    }

    /// Wraps rows already fetched for this page with the counts a caller needs in
    /// order to walk the rest of them.
    pub fn page_of<T>(&self, items: Vec<T>, total: i64) -> OffsetPage<T> {
        let limit = self.limit();
        let total_pages = if limit > 0 {
            total.saturating_add(limit - 1) / limit
        } else {
            0
        };
        let has_more = self.offset().saturating_add(items.len() as i64) < total;

        OffsetPage {
            items,
            total,
            total_pages,
            has_more,
        }
    }
}

/// One page of rows, plus what a caller needs to walk the rest.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct OffsetPage<T> {
    pub items: Vec<T>,
    /// Rows in the table, across every page.
    pub total: i64,
    /// Pages at the requested size; zero when the page size is zero.
    pub total_pages: i64,
    /// Whether any rows follow the ones on this page.
    pub has_more: bool,
}

/// Test-only helpers. Public because the transports' request-level tests need the
/// same migrated in-memory database the model tests use.
#[cfg(any(test, feature = "test-support"))]
pub mod tests {
    use rstest::fixture;
    use sqlx::SqlitePool;

    /// A migrated in-memory database, seeded with `seed` — see [`seeds!`].
    ///
    /// The reference-data migration seeds units and tags for production. Tests start
    /// from an empty table instead and seed their own, because they need control of
    /// the baseline: a test asserting "a rejected name must not insert" is checking
    /// for zero rows, and the fixtures stamp `created_at` with deliberately staggered
    /// offsets that the production seed has no reason to carry.
    #[fixture]
    pub async fn pool(#[default("")] seed: &'static str) -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::raw_sql("DELETE FROM tags; DELETE FROM units;")
            .execute(&pool)
            .await
            .unwrap();
        if !seed.is_empty() {
            sqlx::raw_sql(seed).execute(&pool).await.unwrap();
        }
        pool
    }
}

#[cfg(test)]
mod paging_tests {
    use rstest::rstest;

    use super::*;

    fn paging(number: i64, size: i64) -> Paging {
        Paging { number, size }
    }

    #[rstest]
    #[case::first_page(paging(1, 10), 10, 0)]
    #[case::second_page(paging(2, 10), 10, 10)]
    #[case::page_zero_is_the_first_page(paging(0, 10), 10, 0)]
    #[case::negative_page_number_is_the_first_page(paging(-5, 10), 10, 0)]
    // a negative LIMIT means "no limit" to SQLite, so this must not reach the query
    #[case::negative_size_is_clamped(paging(1, -1), 0, 0)]
    #[case::negative_size_on_a_later_page(paging(9, -1), 0, 0)]
    #[case::zero_size(paging(3, 0), 0, 0)]
    // without the saturating multiply this overflows i64 and panics in debug
    #[case::huge_page_number(paging(i64::MAX, 10), 10, i64::MAX)]
    #[case::huge_page_size(paging(2, i64::MAX), i64::MAX, i64::MAX)]
    #[case::both_huge(paging(i64::MAX, i64::MAX), i64::MAX, i64::MAX)]
    fn limit_and_offset(#[case] paging: Paging, #[case] limit: i64, #[case] offset: i64) {
        assert_eq!(paging.limit(), limit, "limit");
        assert_eq!(paging.offset(), offset, "offset");
    }

    struct PageCase {
        paging: Paging,
        rows: usize,
        total: i64,
        total_pages: i64,
        has_more: bool,
    }

    #[rstest]
    #[case::first_of_four(
        PageCase { paging: paging(1, 6), rows: 6, total: 20, total_pages: 4, has_more: true }
    )]
    #[case::last_partial_page(
        PageCase { paging: paging(4, 6), rows: 2, total: 20, total_pages: 4, has_more: false }
    )]
    #[case::exact_multiple(
        PageCase { paging: paging(2, 10), rows: 10, total: 20, total_pages: 2, has_more: false }
    )]
    #[case::single_page(
        PageCase { paging: paging(1, 100), rows: 20, total: 20, total_pages: 1, has_more: false }
    )]
    #[case::past_the_end(
        PageCase { paging: paging(99, 6), rows: 0, total: 20, total_pages: 4, has_more: false }
    )]
    #[case::empty_table(
        PageCase { paging: paging(1, 10), rows: 0, total: 0, total_pages: 0, has_more: false }
    )]
    // no page size means no pages, but the rows are still out there
    #[case::zero_size(
        PageCase { paging: paging(1, 0), rows: 0, total: 20, total_pages: 0, has_more: true }
    )]
    #[case::negative_size(
        PageCase { paging: paging(1, -1), rows: 0, total: 20, total_pages: 0, has_more: true }
    )]
    #[case::one_row_per_page(
        PageCase { paging: paging(20, 1), rows: 1, total: 20, total_pages: 20, has_more: false }
    )]
    #[case::huge_page_number(
        PageCase { paging: paging(i64::MAX, 6), rows: 0, total: 20, total_pages: 4, has_more: false }
    )]
    fn page_of_counts(#[case] c: PageCase) {
        let page = c.paging.page_of(vec![(); c.rows], c.total);

        assert_eq!(page.items.len(), c.rows);
        assert_eq!(page.total, c.total, "total");
        assert_eq!(page.total_pages, c.total_pages, "total_pages");
        assert_eq!(page.has_more, c.has_more, "has_more");
    }

    /// Walking `has_more` must terminate, land on `total_pages`, and cover every row.
    #[rstest]
    #[case::exact_multiple(20, 5)]
    #[case::partial_last_page(20, 6)]
    #[case::single_row_pages(7, 1)]
    #[case::page_larger_than_the_table(3, 50)]
    #[case::empty_table(0, 10)]
    fn walking_pages_covers_every_row(#[case] total: i64, #[case] size: i64) {
        let mut seen = 0;
        let mut number = 1;

        loop {
            let paging = paging(number, size);
            let remaining = (total - paging.offset()).clamp(0, paging.limit());
            let page = paging.page_of(vec![(); remaining as usize], total);

            seen += page.items.len() as i64;
            if !page.has_more {
                if total > 0 {
                    assert_eq!(page.total_pages, number, "stopped on the wrong page");
                }
                break;
            }
            number += 1;
            assert!(number < 1000, "has_more never cleared");
        }

        assert_eq!(seen, total, "walked {number} pages of {size}");
    }
}
