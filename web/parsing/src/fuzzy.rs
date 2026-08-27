//! Matching what somebody typed against what they have bought before.
//!
//! Here rather than in a transport, and certainly not in both: the phone and the
//! browser must offer the same suggestions for the same letters, and two matchers in
//! two languages agree only until the first one is changed.
//!
//! Pure — strings in, a score out — so every rule below is testable without a
//! database or a request.

/// The best score any match can have. Nothing here returns more than this.
const EXACT: i32 = 1000;
const WORD_START: i32 = 800;
const ANYWHERE: i32 = 600;
const SCATTERED: i32 = 400;

/// How well `candidate` matches `query`, or `None` if it does not match at all.
///
/// Higher is better. The tiers, in order:
///
/// 1. the candidate starts with the query — `mil` → `milk`
/// 2. a word inside it does — `milk` → `almond milk`
/// 3. it contains the query somewhere — `ond` → `almond milk`
/// 4. the query's letters appear in order with gaps — `mlk` → `milk`
///
/// Within a tier, shorter candidates win: someone typing `milk` means `milk` more
/// often than `milk chocolate`, and length is the only evidence available.
///
/// Tier 4 is what makes this fuzzy rather than a prefix search, and it is also the
/// one that lets a short query match almost anything — `ml` is a subsequence of a
/// surprising number of words. That is why it scores below the others rather than
/// being excluded: it turns up, but underneath the matches a person meant.
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.trim().to_lowercase();
    let candidate_lower = candidate.to_lowercase();

    if query.is_empty() {
        return Some(0);
    }

    // Shorter is better within a tier, and a tier is 200 apart, so this can never
    // lift a candidate out of its tier however long it is.
    let brevity = -(candidate.chars().count().min(199) as i32);

    if candidate_lower.starts_with(&query) {
        return Some(EXACT + brevity);
    }

    if candidate_lower
        .split_whitespace()
        .any(|word| word.starts_with(&query))
    {
        return Some(WORD_START + brevity);
    }

    if candidate_lower.contains(&query) {
        return Some(ANYWHERE + brevity);
    }

    is_subsequence(&query, &candidate_lower).then_some(SCATTERED + brevity)
}

/// Whether every character of `query` appears in `candidate`, in order.
fn is_subsequence(query: &str, candidate: &str) -> bool {
    let mut wanted = query.chars().peekable();

    for c in candidate.chars() {
        if wanted.peek() == Some(&c) {
            wanted.next();
        }
    }

    wanted.peek().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// Sorts candidates the way a suggestion list would.
    fn best(query: &str, candidates: &[&str]) -> Vec<String> {
        let mut scored: Vec<_> = candidates
            .iter()
            .filter_map(|c| score(query, c).map(|s| (s, *c)))
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored.into_iter().map(|(_, c)| c.to_string()).collect()
    }

    #[rstest]
    #[case::prefix("mil", "milk")]
    #[case::whole_word("milk", "milk")]
    #[case::inner_word("milk", "almond milk")]
    #[case::substring("ond", "almond milk")]
    #[case::letters_in_order("mlk", "milk")]
    #[case::letters_spread("amk", "almond milk")]
    #[case::case_insensitive("MILK", "milk")]
    #[case::case_insensitive_both("milk", "MILK")]
    #[case::spaces_trimmed("  milk  ", "milk")]
    fn these_match(#[case] query: &str, #[case] candidate: &str) {
        assert!(
            score(query, candidate).is_some(),
            "{query:?} should match {candidate:?}"
        );
    }

    #[rstest]
    #[case::wrong_order("klm", "milk")]
    #[case::extra_letter("milkk", "milk")]
    #[case::nothing_alike("bread", "milk")]
    #[case::longer_than_candidate("milkshake", "milk")]
    fn these_do_not(#[case] query: &str, #[case] candidate: &str) {
        assert_eq!(
            score(query, candidate),
            None,
            "{query:?} should not match {candidate:?}"
        );
    }

    /// The whole point of the tiers: what a person plainly asked for comes first.
    #[test]
    fn a_prefix_beats_a_word_beats_a_scattering() {
        assert_eq!(
            best("milk", &["almond milk", "milk", "marzipan and elderflower kombucha"]),
            vec!["milk", "almond milk", "marzipan and elderflower kombucha"]
        );
    }

    /// Within a tier, the shorter one is the likelier meaning.
    #[test]
    fn shorter_wins_a_tie() {
        assert_eq!(best("milk", &["milk chocolate", "milk"]), vec!["milk", "milk chocolate"]);
    }

    /// Length can never promote a candidate past a better kind of match, however
    /// short it is -- otherwise a stray subsequence would outrank a real prefix.
    #[test]
    fn brevity_cannot_jump_a_tier() {
        let prefix = score("mi", "milk chocolate spread and other things").unwrap();
        let scattered = score("mi", "maize").unwrap();

        assert!(prefix > scattered, "{prefix} should beat {scattered}");
    }

    /// An empty query is not a filter. Callers are expected not to ask, but scoring
    /// everything at zero is a saner answer than matching nothing.
    #[test]
    fn an_empty_query_matches_everything_equally() {
        assert_eq!(score("", "milk"), Some(0));
        assert_eq!(score("   ", "bread"), Some(0));
    }

    /// Non-ASCII goes through the same lowering as everything else.
    #[test]
    fn accents_and_other_alphabets_match() {
        assert!(score("jord", "jordgubbar").is_some());
        assert!(score("ÅNGST", "ångström").is_some());
    }
}
