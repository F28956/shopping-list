//! Reading what somebody typed.
//!
//! Split out of `domain` so that it can be compiled for a phone. Everything else in
//! `domain` reaches a database, which means sqlx, which means tokio, which means a
//! multi-megabyte static library to answer the question "does `2 kg apples` name a
//! unit". These two modules answer it with the standard library and nothing else, and
//! keeping them in a crate with no dependencies is what keeps that true — a build
//! failure is a better guard than a comment asking people not to.
//!
//! `domain` re-exports both, so `crate::quick_add` still resolves inside it and no
//! caller had to change.

pub mod add;
pub mod fuzzy;
pub mod history_rank;
pub mod quick_add;

/// The first letter upper-cased, unless the first word already has a capital.
///
/// Applied where a name is stored rather than where it is shown: three clients
/// capitalising for themselves is three chances to disagree, and the one that gets it
/// wrong is whichever was written last.
///
/// Which is why it is here and not in `domain`. The clients name things for themselves
/// now -- a device with no server has nobody to do it for them -- and this was reached
/// only by the server, so `milk` typed on a phone stayed `milk` and the same keystrokes
/// against a server gave `Milk`. `add::resolve` applies it, so all three agree.
pub fn capitalise(text: &str) -> String {
    let first_word = text.split_whitespace().next().unwrap_or_default();

    // Already spelled deliberately: `iPhone charger`, `BBQ sauce`, `eBay voucher`.
    if first_word.chars().any(char::is_uppercase) {
        return text.to_string();
    }

    let mut chars = text.chars();
    match chars.next() {
        // `to_uppercase` yields more than one char for some letters -- German ß
        // becomes SS -- which is why this is not a single-character swap.
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod capitalise_tests {
    use super::capitalise;
    use rstest::rstest;

    #[rstest]
    #[case::plain("apples", "Apples")]
    #[case::several_words("granny smith apples", "Granny smith apples")]
    #[case::already_capital("Apples", "Apples")]
    // spelled deliberately, and left alone
    #[case::inner_capital("iPhone charger", "iPhone charger")]
    #[case::all_capitals("BBQ sauce", "BBQ sauce")]
    // a capital later on does not excuse a lowercase first word
    #[case::later_capital("bbq sauce for Dad", "Bbq sauce for Dad")]
    #[case::non_ascii("ångström units", "Ångström units")]
    #[case::leading_digit("2 for 1 crisps", "2 for 1 crisps")]
    #[case::empty("", "")]
    fn capitalises(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(capitalise(input), expected);
    }
}
