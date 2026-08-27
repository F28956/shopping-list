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
    let text = input.trim();

    leading(text, known)
        .or_else(|| trailing(text, known))
        .unwrap_or_else(|| QuickAdd {
            name: text.to_string(),
            amount: 1.0,
            unit: None,
        })
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
            unit: None,
        },
        Some((unit, remainder)) => QuickAdd {
            name: remainder.trim().to_string(),
            amount,
            unit: Some(unit),
        },
        None => QuickAdd {
            name: rest.to_string(),
            amount,
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
            unit: None,
        });
    };

    if name.trim().is_empty() {
        // Nothing before the unit, so the unit was the name: `dozen 1`, which is
        // `1 dozen` written backwards and has to mean the same thing.
        return Some(QuickAdd {
            name: head.to_string(),
            amount,
            unit: None,
        });
    }

    Some(QuickAdd {
        name: name.trim().to_string(),
        amount,
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

    Some((unit.clone(), text[unit.len()..].to_string()))
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

    Some((unit.clone(), text[..text.len() - unit.len()].to_string()))
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

        assert_eq!(
            got,
            QuickAdd {
                name: name.to_string(),
                amount,
                unit: unit.map(str::to_string),
            },
            "parsing {input:?}"
        );
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
}
