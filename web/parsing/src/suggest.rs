//! What to offer somebody part-way through typing an item.
//!
//! Two decisions, and they are not the same one: which remembered names are candidates
//! at all — [`crate::fuzzy`] — and what order to put them in. The order is the subtle
//! half, and it was written twice: the server sorted by how well a name matched what
//! was typed and broke ties on how often it is bought, and the phone sorted by how
//! often it is bought and ignored the match entirely. `mil` therefore offered `milk`
//! first on one and `milk chocolate` first on the other, if chocolate was bought more.
//!
//! `fuzzy`'s own comment says two matchers in two languages agree only until the first
//! is changed. The same is true of two policies, and this is the policy.

use crate::fuzzy;
use crate::history_rank::{self, Candidate};

/// How many to offer. More than a handful is a second list on top of the real one.
pub const LIMIT: usize = 6;

/// One remembered name, with what decides its place.
#[derive(Debug, Clone, PartialEq)]
pub struct Remembered {
    /// The spelling to show back — the one last used, not the folded key.
    pub name: String,
    pub uses: i64,
    /// Unix seconds.
    pub last_used_at: i64,
}

/// The names worth offering for `query`, best first.
///
/// `now` is passed in because this crate has no clock, and because a test that says
/// what "recently" means is worth more than one that waits.
///
/// An empty query offers what is bought most, which is what a blank field should show.
pub fn offer(query: &str, candidates: Vec<Remembered>, now: i64) -> Vec<String> {
    // Ranked first, so `position` carries how often and how recently each is bought.
    // That becomes the tie-break; it is not the sort.
    let ranked = history_rank::rank(
        candidates
            .into_iter()
            .map(|c| Candidate {
                uses: c.uses,
                last_used_at: c.last_used_at,
                value: c.name,
            })
            .collect(),
        now,
    );

    let query = query.trim();
    if query.is_empty() {
        return ranked.into_iter().take(LIMIT).collect();
    }

    let mut matches: Vec<(i32, usize, String)> = ranked
        .into_iter()
        .enumerate()
        // Never what has already been typed in full: a suggestion that changes
        // nothing is a row in the way of the ones that would.
        .filter(|(_, name)| !name.eq_ignore_ascii_case(query))
        .filter_map(|(rank, name)| fuzzy::score(query, &name).map(|s| (s, rank, name)))
        .collect();

    // How well it matches decides the order; how often it is bought breaks the ties.
    // `rank` is the position from above, so this keeps the more-used of two equal
    // matches first.
    matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    matches.into_iter().map(|(_, _, name)| name).take(LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remembered(name: &str, uses: i64, days_ago: i64) -> Remembered {
        Remembered {
            name: name.into(),
            uses,
            last_used_at: NOW - days_ago * 86_400,
        }
    }

    const NOW: i64 = 1_756_000_000;

    #[test]
    fn a_better_match_beats_a_more_frequent_one() {
        // The divergence this module exists to end. Sorting by use alone put the
        // staple first whatever was typed; `mil` means milk.
        let offered = offer(
            "mil",
            vec![remembered("milk chocolate", 50, 1), remembered("milk", 2, 30)],
            NOW,
        );
        assert_eq!(offered.first().map(String::as_str), Some("milk"));
    }

    #[test]
    fn how_often_it_is_bought_breaks_a_tie() {
        // Both start with `mi`, so the match tier is equal and use decides.
        let offered = offer(
            "mi",
            vec![remembered("mint", 1, 1), remembered("milk", 40, 1)],
            NOW,
        );
        assert_eq!(offered.first().map(String::as_str), Some("milk"));
    }

    #[test]
    fn what_is_already_typed_in_full_is_not_offered() {
        let offered = offer("milk", vec![remembered("milk", 9, 1)], NOW);
        assert!(offered.is_empty(), "offered a suggestion that changes nothing");
    }

    #[test]
    fn letters_in_order_with_gaps_still_match() {
        // What makes it fuzzy rather than a prefix search.
        let offered = offer("mlk", vec![remembered("milk", 5, 1)], NOW);
        assert_eq!(offered, vec!["milk".to_string()]);
    }

    #[test]
    fn nothing_typed_offers_what_is_bought_most() {
        let offered = offer(
            "",
            vec![remembered("bread", 2, 1), remembered("milk", 40, 1)],
            NOW,
        );
        assert_eq!(offered.first().map(String::as_str), Some("milk"));
    }

    #[test]
    fn no_more_than_a_handful() {
        let many: Vec<Remembered> = (0..20).map(|n| remembered(&format!("milk {n}"), 1, 1)).collect();
        assert_eq!(offer("milk", many, NOW).len(), LIMIT);
    }
}
