#[macro_use]
pub(in crate::models) mod macros;

mod common;
pub mod error;
pub mod history;
pub mod invite;
pub mod item;
pub mod list;
pub mod note;
pub mod tag;
pub mod unit;
pub mod user;

pub use common::{Direction, OffsetPage, OrderBy, Paging};
pub use error::Error;
pub(in crate::models) use error::Result;

#[cfg(any(test, feature = "test-support"))]
pub use common::tests::pool;

/// The seed data, for transports that want to drive their routers against something
/// realistic.
///
/// Exposed as constants rather than through `seeds!`, which is a `macro_rules!`
/// internal to this crate. Order matters — see `models/fixtures/README.md`.
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures {
    pub const USERS: &str = include_str!("models/fixtures/users.sql");
    pub const LISTS: &str = include_str!("models/fixtures/lists.sql");
    pub const UNITS: &str = include_str!("models/fixtures/units.sql");
    pub const ITEMS: &str = include_str!("models/fixtures/items.sql");
    pub const TAGS: &str = include_str!("models/fixtures/tags.sql");
    pub const NOTES: &str = include_str!("models/fixtures/notes.sql");
}

/// The first letter upper-cased, unless the first word already has a capital.
///
/// Applied where a name is stored rather than where it is shown: three clients
/// capitalising for themselves is three chances to disagree, and the one that gets it
/// wrong is whichever was written last.
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
