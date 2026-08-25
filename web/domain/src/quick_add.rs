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
/// The rules, in order:
///
/// 1. A leading number is the amount — `2 apples`, `1.5kg mince`, `500 g flour`.
/// 2. What follows it is the unit, if it names one.
/// 3. Everything left is the name.
///
/// Ambiguity resolves towards keeping the name intact. `1 dozen` has no name left
/// over, so `dozen` is taken as the name rather than leaving an item called nothing.
/// `7 up` becomes seven of "up", which is wrong — but it is the same rule that makes
/// `6 eggs` right, and the editor is one click away.
pub fn parse(input: &str, known: &[String]) -> QuickAdd {
    let text = input.trim();

    let Some((amount, rest)) = leading_number(text) else {
        return QuickAdd {
            name: text.to_string(),
            amount: 1.0,
            unit: None,
        };
    };

    let rest = rest.trim_start();
    if rest.is_empty() {
        // Just a number. It is a name, not a quantity of nothing.
        return QuickAdd {
            name: text.to_string(),
            amount: 1.0,
            unit: None,
        };
    }

    match longest_unit(rest, known) {
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
    }
}

/// Splits a leading decimal off the front. `1.5kg mince` gives `(1.5, "kg mince")`,
/// with no space required — people type it both ways.
fn leading_number(text: &str) -> Option<(f64, &str)> {
    let end = text
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == ',')
        .map(|(i, c)| i + c.len_utf8())
        .last()?;

    // Accept a comma as a decimal point; half the world writes 1,5.
    let number: f64 = text[..end].replace(',', ".").parse().ok()?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    Some((number, &text[end..]))
}

/// The longest known unit that `text` starts with, and what is left after it.
///
/// Longest-first matters: `fl oz` must win over any shorter unit that prefixes it,
/// and a bare `g` must not swallow the start of `garlic`.
fn longest_unit(text: &str, known: &[String]) -> Option<(String, String)> {
    let lowered = text.to_lowercase();

    let mut best: Option<&String> = None;
    for unit in known {
        let u = unit.to_lowercase();
        if !lowered.starts_with(&u) {
            continue;
        }
        // The unit has to end at a word boundary, or `g` matches `garlic`.
        let after = &lowered[u.len()..];
        if !after.is_empty() && !after.starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        if best.is_none_or(|b| b.len() < unit.len()) {
            best = Some(unit);
        }
    }

    let unit = best?;
    Some((unit.clone(), text[unit.len()..].to_string()))
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

    /// The documented wrong answer, kept as a test so that changing the rule is a
    /// deliberate act rather than a surprise.
    #[rstest]
    #[tokio::test]
    async fn a_name_starting_with_a_number_is_read_as_a_quantity() {
        let got = parse("7 up", &units());

        assert_eq!(got.amount, 7.0);
        assert_eq!(got.name, "up");
    }

    #[rstest]
    #[tokio::test]
    async fn an_empty_line_stays_empty() {
        // The CHECK constraint rejects it downstream; this must not invent a name.
        assert_eq!(parse("   ", &units()).name, "");
    }
}
