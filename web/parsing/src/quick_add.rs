//! Reading "2 kg apples" as a person means it.
//!
//! Typing an item should cost one field and one keystroke to submit. The structured
//! fields still exist in the editor for correcting what this got wrong — this is a
//! shortcut, not the only way in, so it is allowed to guess.
//!
//! Pure: it takes the known unit names rather than a database, so every rule here is
//! testable without one.

/// What a line of text turned out to mean.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickAdd {
    pub name: String,
    pub amount: f64,
    /// Whether the **line** gave that amount, or it is the default of one.
    ///
    /// The number alone cannot say: `1 kg flour` and `flour` both come back as one,
    /// and only one of them is somebody stating an amount. A caller that wants to fall
    /// back on what this was last bought in needs to know which it was — see
    /// [`crate::add::resolve`].
    pub stated: bool,
    /// The matched unit, in whatever form the caller supplied it, or `None`.
    pub unit: Option<String>,
}

/// Parses a line, matching units against `known` (longest first, so `fl oz` beats a
/// hypothetical `fl`).
///
/// The quantity may sit at either end, in either order, because people type all of
/// these and mean the same thing:
///
/// * `2 kg apples`
/// * `apples 2 kg`
/// * `apples kg 2`
///
/// A leading quantity is tried first, so a line that could be read both ways is read
/// the way it is written.
///
/// Ambiguity resolves towards keeping the name intact. `1 dozen` has no name left
/// over, so `dozen` is taken as the name rather than leaving an item called nothing,
/// and `dozen 1` does the same.
///
/// The cost is names that end in a number: `7 up` becomes seven of "up", and `factor
/// 50` becomes fifty of "factor". Both are wrong, and both are the same rule that
/// makes `6 eggs` and `apples 6` right. The editor is one tap away.
pub fn parse(input: &str, known: &[String]) -> QuickAdd {
    parse_with(input, known, &[])
}

/// As [`parse`], and also reads a unit written with no number in front of it.
///
/// `pint milk` is one pint of milk and everybody knows it, but only some units can be
/// read that way. Half of them are also the first word of ordinary things to buy --
/// `can opener`, `tin foil`, `box grater`, `tube socks`, `pound cake` -- and reading
/// those as a quantity would be worse than not helping. So `standalone` names the ones
/// that may, and it comes from the units table rather than from a guess here: it is a
/// fact about each unit, and the server and the clients read it from the same place.
///
/// A number still wins wherever there is one. This only runs when neither end has one.
pub fn parse_with(input: &str, known: &[String], standalone: &[String]) -> QuickAdd {
    let text = input.trim();

    leading(text, known)
        .or_else(|| trailing(text, known))
        .or_else(|| bare_unit(text, standalone))
        .unwrap_or_else(|| QuickAdd {
            name: text.to_string(),
            amount: 1.0,
            stated: false,
            unit: None,
        })
}

/// `pint milk` — a unit that may stand alone, then the name.
///
/// Leading only. A trailing one (`milk pint`) is left alone deliberately: it reads far
/// less like a quantity, and `orange squash pint` is the shape of a name more often
/// than it is the shape of an order.
fn bare_unit(text: &str, standalone: &[String]) -> Option<QuickAdd> {
    // Longest first, so `fl oz` beats a hypothetical `fl` -- the same reason `leading`
    // sorts them.
    let mut candidates: Vec<&String> = standalone.iter().collect();
    candidates.sort_by_key(|u| std::cmp::Reverse(u.len()));

    for unit in candidates {
        let Some(rest) = strip_leading_word(text, unit) else { continue };
        if rest.is_empty() {
            // `pint` on its own is a thing somebody wants one of, not a quantity of
            // nothing. Left as a name, the same way `1 dozen` is.
            continue;
        }
        return Some(QuickAdd {
            name: rest.to_string(),
            amount: 1.0,
            // Not stated: nobody wrote a number, so how much you usually buy still
            // applies -- see `crate::add::amount_for`.
            stated: false,
            unit: Some(unit.clone()),
        });
    }
    None
}

/// `text` with `word` removed from the front, if it is there as a whole word.
fn strip_leading_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let rest = text.get(..word.len()).filter(|head| head.eq_ignore_ascii_case(word))?;
    let _ = rest;
    let after = &text[word.len()..];
    // A word boundary, so `mint` is not `m` followed by `int`.
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some(after.trim_start())
}

/// `2 kg apples` — a number, then perhaps a unit, then the name.
fn leading(text: &str, known: &[String]) -> Option<QuickAdd> {
    let (amount, rest) = leading_number(text)?;
    let rest = rest.trim_start();

    // Just a number. It is a name, not a quantity of nothing.
    if rest.is_empty() {
        return None;
    }

    Some(match longest_unit(rest, known) {
        // A unit with nothing after it means the "unit" was the name: `1 dozen`.
        Some((_, remainder)) if remainder.trim().is_empty() => QuickAdd {
            name: rest.to_string(),
            amount,
            stated: true,
            unit: None,
        },
        Some((unit, remainder)) => QuickAdd {
            name: remainder.trim().to_string(),
            amount,
            stated: true,
            unit: Some(unit),
        },
        None => QuickAdd {
            name: rest.to_string(),
            amount,
            stated: true,
            unit: None,
        },
    })
}

/// `apples 2 kg` and `apples kg 2` — the name first, the quantity after it.
fn trailing(text: &str, known: &[String]) -> Option<QuickAdd> {
    // `apples 2 kg`: the unit is last, the number just before it.
    if let Some((unit, head)) = trailing_unit(text, known)
        && let Some((amount, name)) = trailing_number(head.trim_end())
        && !name.trim().is_empty()
    {
        return Some(QuickAdd {
            name: name.trim().to_string(),
            amount,
            stated: true,
            unit: Some(unit),
        });
    }

    // `apples kg 2`, and `apples 2` with no unit at all.
    let (amount, head) = trailing_number(text)?;
    let head = head.trim_end();
    if head.is_empty() {
        return None;
    }

    let Some((unit, name)) = trailing_unit(head, known) else {
        return Some(QuickAdd {
            name: head.to_string(),
            amount,
            stated: true,
            unit: None,
        });
    };

    if name.trim().is_empty() {
        // Nothing before the unit, so the unit was the name: `dozen 1`, which is
        // `1 dozen` written backwards and has to mean the same thing.
        return Some(QuickAdd {
            name: head.to_string(),
            amount,
            stated: true,
            unit: None,
        });
    }

    Some(QuickAdd {
        name: name.trim().to_string(),
        amount,
        stated: true,
        unit: Some(unit),
    })
}

/// Splits a leading decimal off the front. `1.5kg mince` gives `(1.5, "kg mince")`,
/// with no space required — people type it both ways.
fn leading_number(text: &str) -> Option<(f64, &str)> {
    let end = text
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == ',')
        .map(|(i, c)| i + c.len_utf8())
        .last()?;

    number(&text[..end]).map(|n| (n, &text[end..]))
}

/// Splits a trailing decimal off the end. `mince 1.5` gives `(1.5, "mince ")`.
///
/// The number has to be a word of its own. Glued to what precedes it, it is part of
/// the name: `item-1`, `omega-3` and `no5` are one thing each, not one of something
/// called `item-`. The leading form needs no such rule, because nothing can precede
/// a number at the start of the line.
fn trailing_number(text: &str) -> Option<(f64, &str)> {
    let start = text
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == ',')
        .map(|(i, _)| i)
        .last()?;

    let before = &text[..start];
    if !before.is_empty() && !before.ends_with(|c: char| c.is_whitespace()) {
        return None;
    }

    number(&text[start..]).map(|n| (n, before))
}

/// A quantity, or nothing. Zero and negatives are not quantities — the `CHECK` on
/// the column would reject them anyway, so reading one as an amount would turn a
/// typed line into a failed add rather than an oddly-named item.
fn number(text: &str) -> Option<f64> {
    // Accept a comma as a decimal point; half the world writes 1,5.
    let value: f64 = text.replace(',', ".").parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

/// The longest known unit that `text` starts with, and what is left after it.
///
/// Longest-first matters: `fl oz` must win over any shorter unit that prefixes it,
/// and a bare `g` must not swallow the start of `garlic`.
fn longest_unit(text: &str, known: &[String]) -> Option<(String, String)> {
    let lowered = text.to_lowercase();

    let unit = best_unit(known, |u| {
        // The unit has to end at a word boundary, or `g` matches `garlic`.
        lowered.strip_prefix(u).is_some_and(|after| {
            after.is_empty() || after.starts_with(|c: char| c.is_whitespace())
        })
    })?;

    // `unit.len()` is a length in the *lowered* string, and lowering is not
    // length-preserving -- see `lowered_prefix_end`.
    let end = lowered_prefix_end(text, unit)?;
    Some((unit.clone(), text[end..].to_string()))
}

/// The longest known unit that `text` ends with, and what comes before it.
///
/// The mirror of [`longest_unit`], boundary rule included: without it a bare `g`
/// matches the end of `nutmeg`.
fn trailing_unit(text: &str, known: &[String]) -> Option<(String, String)> {
    let lowered = text.to_lowercase();

    let unit = best_unit(known, |u| {
        lowered.strip_suffix(u).is_some_and(|before| {
            // A digit counts as a boundary as well as a space, because `1.5kg` is
            // written without one and the leading form already accepts it. `nutmeg`
            // is still safe: what precedes the `g` there is `nutme`.
            before.is_empty()
                || before.ends_with(|c: char| c.is_whitespace() || c.is_ascii_digit())
        })
    })?;

    let start = lowered_suffix_start(text, unit)?;
    Some((unit.clone(), text[..start].to_string()))
}

/// Where a prefix that lower-cases to `lowered` ends in the original `text`.
///
/// `str::to_lowercase` is not length-preserving: `\u{212A}` KELVIN SIGN is three bytes
/// and lowers to a one-byte `k`, and `\u{130}` grows instead of shrinking. So a match
/// found in the lowered string says nothing about where to cut the original -- and both
/// of these cut the original using the lowered string's byte count. `2 \u{212A}g milk`,
/// which is what an ordinary "2 Kg milk" becomes after a paste from the wrong place,
/// sliced through the middle of a character and panicked. The mirror below could
/// underflow `usize` and ask for a byte index near `2^64` instead.
///
/// A panic here is not a crash report on a server -- `catch_unwind` was added to the
/// FFI boundaries at the same time as this, and before that an unwind across
/// `extern "C"` was undefined behaviour in every app that links the parser.
///
/// So: walk the original, lowering as we go, and report a boundary that actually exists
/// in it. `None` where the prefix ends inside a character that lowered to several --
/// there is no such boundary, and refusing to split is the honest answer.
fn lowered_prefix_end(text: &str, lowered: &str) -> Option<usize> {
    let mut far = String::with_capacity(lowered.len());
    for (at, c) in text.char_indices() {
        far.extend(c.to_lowercase());
        if far.len() > lowered.len() {
            return None;
        }
        if far == lowered {
            return Some(at + c.len_utf8());
        }
    }
    None
}

/// Where a suffix that lower-cases to `lowered` begins in the original `text`.
///
/// The mirror of [`lowered_prefix_end`], and the same reasoning.
fn lowered_suffix_start(text: &str, lowered: &str) -> Option<usize> {
    let mut far = String::with_capacity(lowered.len());
    for (at, c) in text.char_indices().rev() {
        far.insert_str(0, &c.to_lowercase().collect::<String>());
        if far.len() > lowered.len() {
            return None;
        }
        if far == lowered {
            return Some(at);
        }
    }
    None
}

/// The longest unit matching `matches`, compared in lower case.
fn best_unit(known: &[String], matches: impl Fn(&str) -> bool) -> Option<&String> {
    known
        .iter()
        .filter(|unit| matches(&unit.to_lowercase()))
        .max_by_key(|unit| unit.len())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn units() -> Vec<String> {
        [
            "kg", "g", "ml", "litre", "fl oz", "dozen", "pack", "unit", "tin", "cup",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[rstest]
    // the plain case: no number, no unit
    #[case("Apples", "Apples", 1.0, None)]
    #[case("  Apples  ", "Apples", 1.0, None)]
    // a leading number is an amount
    #[case("6 eggs", "eggs", 6.0, None)]
    #[case("2 kg apples", "apples", 2.0, Some("kg"))]
    // people type it without the space
    #[case("500g flour", "flour", 500.0, Some("g"))]
    #[case("1.5kg mince", "mince", 1.5, Some("kg"))]
    // and with a comma, because half the world does
    #[case("1,5 kg mince", "mince", 1.5, Some("kg"))]
    // multi-word units have to win over their own prefixes
    #[case("4 fl oz cream", "cream", 4.0, Some("fl oz"))]
    // a unit must end at a word boundary, or `g` eats `garlic`
    #[case("2 garlic bulbs", "garlic bulbs", 2.0, None)]
    #[case("3 kgs of something", "kgs of something", 3.0, None)]
    // a unit with nothing after it is the name
    #[case("1 dozen", "dozen", 1.0, None)]
    #[case("2 packs", "packs", 2.0, None)]
    // names that are only a number stay names
    #[case("7", "7", 1.0, None)]
    // non-ASCII survives
    #[case("2 kg jordgubbar", "jordgubbar", 2.0, Some("kg"))]
    #[case("Ångström units", "Ångström units", 1.0, None)]
    // zero and negatives are not amounts; the CHECK would reject them anyway
    #[case("0 apples", "0 apples", 1.0, None)]
    #[case("-2 apples", "-2 apples", 1.0, None)]
    // the quantity may sit at the end instead, in either order
    #[case("apples 2 kg", "apples", 2.0, Some("kg"))]
    #[case("apples kg 2", "apples", 2.0, Some("kg"))]
    #[case("apples 2", "apples", 2.0, None)]
    // ... and the same spellings the leading form allows
    #[case("mince 1.5kg", "mince", 1.5, Some("kg"))]
    #[case("mince 1,5 kg", "mince", 1.5, Some("kg"))]
    #[case("cream 4 fl oz", "cream", 4.0, Some("fl oz"))]
    #[case("cream fl oz 4", "cream", 4.0, Some("fl oz"))]
    // a unit with no name before it is the name, the same as `1 dozen`
    #[case("dozen 1", "dozen", 1.0, None)]
    // the boundary rule mirrored: a bare `g` must not match the end of `nutmeg`
    #[case("nutmeg 2", "nutmeg", 2.0, None)]
    #[case("2 nutmeg", "nutmeg", 2.0, None)]
    // a trailing unit with no quantity anywhere is part of the name
    #[case("apple kg", "apple kg", 1.0, None)]
    // a leading quantity wins, so a line is read the way it is written
    #[case("2 kg apples 3", "apples 3", 2.0, Some("kg"))]
    // and zero is not an amount at either end
    #[case("apples 0", "apples 0", 1.0, None)]
    // a number glued to the name is part of it, not a quantity
    #[case("item-1", "item-1", 1.0, None)]
    #[case("omega-3", "omega-3", 1.0, None)]
    #[case("no5", "no5", 1.0, None)]
    #[case("no5 kg", "no5 kg", 1.0, None)]
    fn parses(
        #[case] input: &str,
        #[case] name: &str,
        #[case] amount: f64,
        #[case] unit: Option<&str>,
    ) {
        let got = parse(input, &units());

        // Field by field rather than whole-struct: these cases are about what the
        // words mean, and `stated` is a different question with its own test below.
        assert_eq!(got.name, name, "name, parsing {input:?}");
        assert_eq!(got.amount, amount, "amount, parsing {input:?}");
        assert_eq!(got.unit.as_deref(), unit, "unit, parsing {input:?}");
    }

    /// Whether the line gave an amount, as opposed to being handed the default.
    ///
    /// The number cannot say on its own: `1 kg flour` and `flour` are both one, and
    /// only the first is somebody stating an amount. What hangs on it is whether
    /// `add::resolve` may fall back to how much you usually buy.
    #[rstest]
    #[case("2 kg apples", true)]
    #[case("apples 2", true)]
    #[case("1 kg flour", true)]
    #[case("apples", false)]
    #[case("fresh bread", false)]
    #[case("", false)]
    fn says_whether_the_amount_was_given(#[case] input: &str, #[case] stated: bool) {
        assert_eq!(parse(input, &units()).stated, stated, "parsing {input:?}");
    }

    /// The documented wrong answers, kept as tests so that changing the rule is a
    /// deliberate act rather than a surprise. Both are the price of the rule that
    /// makes `6 eggs` and `apples 6` right.
    #[rstest]
    #[case::leading("7 up", 7.0, "up")]
    #[case::trailing("factor 50", 50.0, "factor")]
    fn a_name_that_is_partly_a_number_is_read_as_a_quantity(
        #[case] input: &str,
        #[case] amount: f64,
        #[case] name: &str,
    ) {
        let got = parse(input, &units());

        assert_eq!(got.amount, amount);
        assert_eq!(got.name, name);
    }

    #[rstest]
    // Plain `#[test]`, unlike its neighbours in `domain`: nothing here is async, and a
    // runtime dependency for a crate the phones compile is not worth the symmetry.
    #[test]
    fn an_empty_line_stays_empty() {
        // The CHECK constraint rejects it downstream; this must not invent a name.
        assert_eq!(parse("   ", &units()).name, "");
    }

    fn standalone() -> Vec<String> {
        ["kg", "pint", "litre", "fl oz", "dozen"].iter().map(|s| s.to_string()).collect()
    }

    /// The shared fixture plus `pint`, which these cases are mostly about.
    fn units_including_pint() -> Vec<String> {
        let mut all = units();
        all.push("pint".to_string());
        all
    }

    /// A unit with no number in front of it.
    #[rstest]
    // The case this exists for.
    #[case("pint milk", "milk", 1.0, Some("pint"))]
    #[case("kg apples", "apples", 1.0, Some("kg"))]
    #[case("dozen eggs", "eggs", 1.0, Some("dozen"))]
    // Case and spacing, as everywhere else here.
    #[case("PINT milk", "milk", 1.0, Some("pint"))]
    #[case("  pint   milk  ", "milk", 1.0, Some("pint"))]
    // Two words, and the longer match wins.
    #[case("fl oz cream", "cream", 1.0, Some("fl oz"))]
    // A number still wins wherever there is one.
    #[case("2 pint milk", "milk", 2.0, Some("pint"))]
    // Whole words only: `mint` is not `m` and then `int`.
    #[case("mint sauce", "mint sauce", 1.0, None)]
    // Nothing left over is a name, not a quantity of nothing -- as `1 dozen` is.
    #[case("pint", "pint", 1.0, None)]
    fn reads_a_unit_written_without_a_number(
        #[case] input: &str,
        #[case] name: &str,
        #[case] amount: f64,
        #[case] unit: Option<&str>,
    ) {
        let got = parse_with(input, &units_including_pint(), &standalone());
        assert_eq!(got.name, name, "name, parsing {input:?}");
        assert_eq!(got.amount, amount, "amount, parsing {input:?}");
        assert_eq!(got.unit.as_deref(), unit, "unit, parsing {input:?}");
    }

    /// The units that may **not** stand alone, which is most of them.
    ///
    /// These are the reason the rule is per unit rather than for every unit: each of
    /// these is an ordinary thing to buy whose name happens to start with a unit, and
    /// reading it as a quantity would be worse than not helping at all.
    #[rstest]
    #[case("can opener")]
    #[case("tin foil")]
    #[case("box grater")]
    #[case("tube socks")]
    #[case("bag clips")]
    #[case("roll mat")]
    #[case("pound cake")]
    #[case("cup cakes")]
    fn leaves_names_that_begin_with_a_unit_alone(#[case] input: &str) {
        // The real table, so this fails the day somebody marks one of these `bare`.
        let all: Vec<String> = [
            "unit", "pair", "dozen", "pack", "box", "bag", "bottle", "can", "jar",
            "tin", "tube", "sachet", "roll", "bunch", "punnet", "loaf", "slice", "g",
            "kg", "oz", "pound", "ml", "litre", "fl oz", "pint", "gallon", "tsp",
            "tbsp", "cup", "cm", "m",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let bare: Vec<String> = [
            "g", "kg", "oz", "ml", "litre", "fl oz", "pint", "gallon", "tsp", "tbsp",
            "cm", "m", "dozen",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let got = parse_with(input, &all, &bare);
        assert_eq!(got.name, input, "{input:?} was read as a quantity");
        assert_eq!(got.unit, None, "{input:?} was read as a quantity");
    }
}

#[cfg(test)]
mod lowering_is_not_length_preserving {
    use super::*;

    fn seeded() -> Vec<String> {
        ["g", "kg", "ml", "litre", "pack", "box", "tin", "m", "cm"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// The one that reaches an ordinary person: `\u{212A}` KELVIN SIGN looks exactly
    /// like a capital K and arrives from a paste. Three bytes in, one byte lowered, so
    /// the old code cut `2 \u{212A}g milk` through the middle of a character.
    ///
    /// It need not parse as a quantity -- what it must not do is panic, and what it
    /// must not do is lose the line.
    #[test]
    fn a_kelvin_sign_where_a_k_was_meant_does_not_panic() {
        let parsed = parse("2 \u{212A}g milk", &seeded());
        assert!(
            parsed.name.contains("milk"),
            "the line was lost: {:?}",
            parsed.name
        );
    }

    /// The mirror, which could underflow `usize` and ask for a byte index near 2^64.
    #[test]
    fn a_trailing_one_does_not_underflow() {
        let _ = parse("milk 2 \u{212A}g", &seeded());
        let _ = parse("\u{00DF}", &["\u{1E9E}".to_string()]);
        let _ = parse("\u{212A}", &["k".to_string()]);
    }

    /// A character that lower-cases to *more* than itself, which is the other
    /// direction: `\u{130}` is two bytes and lowers to three.
    #[test]
    fn a_character_that_grows_when_lowered_does_not_panic() {
        let _ = parse("2 \u{130}g milk", &seeded());
        let _ = parse("\u{130}", &["i".to_string(), "i\u{307}".to_string()]);
    }

    /// And the boundary is the one in the *original* text, not a byte count taken from
    /// the lowered copy. `\u{212A}g` is four bytes; `kg` is two.
    #[test]
    fn the_boundary_comes_from_the_original() {
        assert_eq!(lowered_prefix_end("\u{212A}g milk", "kg"), Some(4));
        assert_eq!(lowered_suffix_start("milk \u{212A}g", "kg"), Some(5));
        // No boundary exists where one character lowered to several.
        assert_eq!(lowered_prefix_end("\u{130}x", "i"), None);
    }

    /// The ordinary case is untouched.
    #[test]
    fn plain_ascii_still_splits_where_it_always_did() {
        let parsed = parse("2 kg apples", &seeded());
        assert_eq!(parsed.name, "apples");
        assert_eq!(parsed.unit.as_deref(), Some("kg"));

        let trailing = parse("apples 2 kg", &seeded());
        assert_eq!(trailing.name, "apples");
        assert_eq!(trailing.unit.as_deref(), Some("kg"));

        // And capitals, which is the path that lowering exists for.
        let shouted = parse("2 KG apples", &seeded());
        assert_eq!(shouted.unit.as_deref(), Some("kg"), "an uppercase unit stopped matching");
    }
}
