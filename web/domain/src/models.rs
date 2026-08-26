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

/// A fresh v4 UUID, lowercase hex with the usual dashes.
///
/// Written out by hand rather than pulled in with the `uuid` crate: this needs one
/// function of it, the format is sixteen bytes and two nibbles, and the crate would
/// arrive with a serde surface and a feature matrix for something the models already
/// have the randomness for.
///
/// The generator is `rand::rng()`, which is seeded from the operating system. A
/// device mints these too, and two devices that never meet must not collide, so
/// "random enough for a shopping list" is not the bar — 122 bits is.
pub fn new_uuid() -> String {
    use rand::Rng;

    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);

    // Version 4 in the high nibble of byte 6, and the RFC 4122 variant in the top two
    // bits of byte 8. Everything else stays random.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A UUID as it will be stored: lowercase, dashes where they belong.
///
/// Shape only. The version and variant nibbles are deliberately not checked — a
/// device that mints a v7 for its own reasons is naming a row, not making a claim
/// about how it drew the bits, and refusing it would buy nothing. What is checked is
/// what an index depends on: a fixed length, hex digits, and one spelling per value,
/// so `A1B2…` and `a1b2…` cannot become two rows for one thing.
pub fn parse_uuid(text: &str) -> std::result::Result<String, Error> {
    const DASHES: [usize; 4] = [8, 13, 18, 23];

    let text = text.trim();

    let shaped = text.len() == 36
        && DASHES.iter().all(|&i| text.as_bytes()[i] == b'-')
        && text
            .char_indices()
            .filter(|(i, _)| !DASHES.contains(i))
            .all(|(_, c)| c.is_ascii_hexdigit());

    if !shaped {
        return Err(Error::InvalidInput);
    }

    Ok(text.to_ascii_lowercase())
}

#[cfg(test)]
mod uuid_tests {
    use super::{new_uuid, parse_uuid};
    use rstest::rstest;

    #[test]
    fn mints_something_it_would_accept_back() {
        let minted = new_uuid();
        assert_eq!(parse_uuid(&minted).unwrap(), minted);
    }

    #[test]
    fn mints_a_different_one_every_time() {
        let many: std::collections::HashSet<String> = (0..1_000).map(|_| new_uuid()).collect();
        assert_eq!(many.len(), 1_000);
    }

    #[test]
    fn mints_version_four() {
        let minted = new_uuid();
        assert_eq!(&minted[14..15], "4", "{minted}");
        assert!("89ab".contains(&minted[19..20]), "{minted}");
    }

    #[rstest]
    #[case::lowercased("A1B2C3D4-E5F6-4789-A012-3456789ABCDE", "a1b2c3d4-e5f6-4789-a012-3456789abcde")]
    #[case::padding_comes_off("  a1b2c3d4-e5f6-4789-a012-3456789abcde  ", "a1b2c3d4-e5f6-4789-a012-3456789abcde")]
    // A device that mints a v7 is naming a row, not claiming how it drew the bits.
    #[case::any_version("a1b2c3d4-e5f6-7789-c012-3456789abcde", "a1b2c3d4-e5f6-7789-c012-3456789abcde")]
    fn accepts(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(parse_uuid(input).unwrap(), expected);
    }

    #[rstest]
    #[case::empty("")]
    #[case::too_short("a1b2c3d4-e5f6-4789-a012-3456789abcd")]
    #[case::too_long("a1b2c3d4-e5f6-4789-a012-3456789abcdef")]
    #[case::no_dashes("a1b2c3d4e5f647890a123456789abcde")]
    #[case::dashes_in_the_wrong_places("a1b2c3d4e-5f6-4789-a012-3456789abcde")]
    #[case::not_hex("g1b2c3d4-e5f6-4789-a012-3456789abcde")]
    // 36 characters and dashes in the right places, but the gaps are not hex
    #[case::right_shape_wrong_alphabet("zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz")]
    fn refuses(#[case] input: &str) {
        assert!(parse_uuid(input).is_err(), "{input:?} should not parse");
    }
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
