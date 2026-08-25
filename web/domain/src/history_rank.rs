//! Which remembered items to offer first.
//!
//! Pure, and separate from the query that fetches them, for the same reason
//! [`crate::quick_add`] is: this is a policy, and a policy deserves tests that do not
//! need a database. SQL bounds the candidate set; the order is decided here.
//!
//! It is not done in SQL for a second reason — the obvious formula wants `exp()`, and
//! that needs `SQLITE_ENABLE_MATH_FUNCTIONS`, which a bundled SQLite may not carry.

/// How long it takes for a use to count for half as much.
///
/// Thirty days: long enough that a weekly staple stays near the top through a missed
/// shop, short enough that something bought once in a burst last month falls away.
pub const HALF_LIFE_DAYS: f64 = 30.0;

const SECONDS_PER_DAY: f64 = 86_400.0;

/// A candidate, as it comes out of the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate<T> {
    pub value: T,
    pub uses: i64,
    /// Unix seconds.
    pub last_used_at: i64,
}

/// Sorts candidates by how likely they are to be what someone is about to type:
/// often bought, recently bought, in that combination.
///
/// A single item bought yesterday should not outrank a staple bought weekly for a
/// year, and a staple abandoned six months ago should not outrank what was bought
/// last week. Multiplying the count by an exponential decay of its age does both.
///
/// Ties break on recency, then on the value itself so the order is stable — an
/// unstable suggestion list is worse than a slightly wrong one, because the thing you
/// were reaching for moves.
pub fn rank<T: Ord + Clone>(mut candidates: Vec<Candidate<T>>, now: i64) -> Vec<T> {
    candidates.sort_by(|a, b| {
        score(b, now)
            .total_cmp(&score(a, now))
            .then(b.last_used_at.cmp(&a.last_used_at))
            .then(a.value.cmp(&b.value))
    });
    candidates.into_iter().map(|c| c.value).collect()
}

/// `uses`, halved for every [`HALF_LIFE_DAYS`] since it was last used.
///
/// Future timestamps score as if they were now rather than blowing up: a clock skew
/// on one row should not park it at the top of the list forever.
pub fn score<T>(c: &Candidate<T>, now: i64) -> f64 {
    let age_days = ((now - c.last_used_at).max(0) as f64) / SECONDS_PER_DAY;
    c.uses as f64 * 0.5_f64.powf(age_days / HALF_LIFE_DAYS)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn days_ago(n: f64) -> i64 {
        NOW - (n * SECONDS_PER_DAY) as i64
    }

    fn candidate(value: &str, uses: i64, age_days: f64) -> Candidate<String> {
        Candidate {
            value: value.to_string(),
            uses,
            last_used_at: days_ago(age_days),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn more_uses_wins_when_ages_match() {
        let ranked = rank(
            vec![candidate("rare", 1, 7.0), candidate("staple", 40, 7.0)],
            NOW,
        );

        assert_eq!(ranked, vec!["staple", "rare"]);
    }

    #[rstest]
    #[tokio::test]
    async fn recent_wins_when_counts_match() {
        let ranked = rank(
            vec![candidate("stale", 5, 120.0), candidate("fresh", 5, 1.0)],
            NOW,
        );

        assert_eq!(ranked, vec!["fresh", "stale"]);
    }

    /// The case the whole formula exists for: a weekly staple beats a one-week burst.
    #[rstest]
    #[tokio::test]
    async fn a_long_habit_beats_a_short_burst() {
        let ranked = rank(
            vec![
                // bought five times in the last few days and then never again
                candidate("burst", 5, 3.0),
                // bought weekly for a year, last week
                candidate("milk", 52, 7.0),
            ],
            NOW,
        );

        assert_eq!(ranked, vec!["milk", "burst"]);
    }

    /// And the other direction: an abandoned staple sinks below current shopping.
    #[rstest]
    #[tokio::test]
    async fn an_abandoned_staple_sinks() {
        let ranked = rank(
            vec![
                // bought weekly for a year, but not for six months
                candidate("old-favourite", 52, 180.0),
                // bought twice, this week
                candidate("current", 2, 2.0),
            ],
            NOW,
        );

        assert_eq!(ranked, vec!["current", "old-favourite"]);
    }

    #[rstest]
    #[tokio::test]
    async fn a_use_is_worth_half_after_one_half_life() {
        let fresh = candidate("a", 8, 0.0);
        let aged = candidate("b", 8, HALF_LIFE_DAYS);

        let ratio = score(&aged, NOW) / score(&fresh, NOW);

        assert!(
            (ratio - 0.5).abs() < 1e-9,
            "expected a half-life to halve the score, got {ratio}"
        );
    }

    /// A clock skew must not park a row at the top forever.
    #[rstest]
    #[tokio::test]
    async fn a_future_timestamp_scores_as_now() {
        let future = Candidate {
            value: "tomorrow".to_string(),
            uses: 1,
            last_used_at: NOW + 10 * SECONDS_PER_DAY as i64,
        };
        let now = candidate("today", 1, 0.0);

        assert!((score(&future, NOW) - score(&now, NOW)).abs() < 1e-9);
    }

    /// Stability matters: a suggestion list that reshuffles between renders moves the
    /// thing the person was reaching for.
    #[rstest]
    #[tokio::test]
    async fn equal_candidates_keep_a_stable_order() {
        let a = rank(
            vec![candidate("pears", 3, 5.0), candidate("apples", 3, 5.0)],
            NOW,
        );
        let b = rank(
            vec![candidate("apples", 3, 5.0), candidate("pears", 3, 5.0)],
            NOW,
        );

        assert_eq!(a, b);
        assert_eq!(a, vec!["apples", "pears"]);
    }

    #[rstest]
    #[tokio::test]
    async fn nothing_ranks_to_nothing() {
        assert!(rank(Vec::<Candidate<String>>::new(), NOW).is_empty());
    }
}
