//! What a typed line should do to a list.
//!
//! Reading the line is [`crate::quick_add`]'s job. This is everything that comes
//! after: which unit it ends up in, whether it is a row the list already has, and what
//! a re-add does to one that was crossed off.
//!
//! These are rules, not lookups. They were the server's alone, which was fine while
//! the server was the only thing that added an item — and stopped being fine when the
//! clients started doing it themselves with no server to correct them. Written out a
//! second time in Swift they immediately drifted: `milk`, `milk` and `Milk` became
//! three rows on a phone and one row on a server.
//!
//! So they live here, beside the parser, and everyone calls them: the server directly,
//! the phones and the Mac through the C boundary, and Android through JNI. The caller
//! brings the data — this crate has no database and no clock — and gets back a
//! decision it can carry out however it stores things.

use crate::quick_add;

/// A row the list already has, as much of it as the rules need.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The name that travels — see the clients' outbox. Opaque here.
    pub uuid: String,
    pub name: String,
    pub unit_id: Option<i64>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub id: i64,
    pub name: String,
    /// Whether this unit means something written with no number in front of it --
    /// `pint milk`. See [`crate::quick_add::parse_with`] for why it is per unit.
    pub bare: bool,
}

/// What this list's history knows about a name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Remembered {
    /// The name this is remembered under, as stored. Folded before it is matched, so
    /// it does not matter whether the caller has folded it already.
    pub name: String,
    pub unit_id: Option<i64>,
    /// How much of it was last bought.
    ///
    /// `2 kg apples` once, then `apples`, should be two kilos again -- a shopping list
    /// that forgets how much you buy makes you say it every week. The line still wins
    /// when it says a number, because that is somebody stating one.
    pub amount: Option<f64>,
    pub tag_ids: Vec<i64>,
}

/// What to do about the line.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// The list already wants this. Nothing is created.
    ///
    /// `put_back` when the row was crossed off: adding something you have already
    /// ticked off is how you say you need it after all, so it comes back — with the
    /// amount it had, untouched.
    Existing { uuid: String, put_back: bool },
    /// Nothing on the list matches, so this is a new row.
    New {
        name: String,
        amount: f64,
        unit_id: Option<i64>,
        /// Where the history says this belongs, so a re-added item files itself.
        tag_ids: Vec<i64>,
    },
}

/// The unit a line ends up in.
///
/// In order: what the line spelled, then what this name was last bought in, then
/// `unit` — counted rather than measured is still a unit, and `unit` is the one that
/// says so. Left as `None`, `milk` and `1 unit milk` are different units and therefore
/// different rows, and the list grows a near-duplicate nothing will ever merge.
pub fn unit_for(spelled: Option<&str>, remembered: Option<&Remembered>, units: &[Unit]) -> Option<i64> {
    let by_name = |wanted: &str| {
        units
            .iter()
            .find(|u| u.name.eq_ignore_ascii_case(wanted))
            .map(|u| u.id)
    };

    spelled
        .and_then(by_name)
        .or_else(|| remembered.and_then(|r| r.unit_id))
        .or_else(|| by_name("unit"))
}

/// How much a line asks for.
///
/// The line first, then the memory, then one. Somebody who wrote a number meant it;
/// somebody who did not is being handed back what they usually buy -- `2 kg apples`
/// once, then `apples`, is two kilos again. A list that forgets how much you buy makes
/// you say it every week.
///
/// `stated` is why [`crate::quick_add::QuickAdd`] carries that flag: the number alone
/// cannot say, because `1 kg flour` and `flour` are both one.
pub fn amount_for(parsed: &quick_add::QuickAdd, remembered: Option<&Remembered>) -> f64 {
    if parsed.stated {
        return parsed.amount;
    }
    remembered
        .and_then(|r| r.amount)
        .unwrap_or(parsed.amount)
}

/// The one key a name is remembered under.
///
/// Trimmed and lowercased, so `Milk`, `milk ` and `MILK` are one memory. Shared
/// because both ends look things up by it and a fold written twice folds differently:
/// the phone lowercased without trimming, which is the same key for every name anybody
/// actually types and a different one the moment somebody types a trailing space.
pub fn fold(name: &str) -> String {
    name.trim().to_lowercase()
}

/// What the history knows about a name, if anything.
///
/// **The name is the parsed one, not the line.** That was the bug this exists to stop:
/// the phone looked up what somebody typed, so `2 kg apples` went looking for a memory
/// of "2 kg apples" and never found "apples" -- the history it had just been given was
/// consulted for every bare word and for nothing else. Reading the line is what tells
/// you the name, so the lookup cannot happen before it.
pub fn recall<'a>(history: &'a [Remembered], name: &str) -> Option<&'a Remembered> {
    let wanted = fold(name);
    history.iter().find(|r| fold(&r.name) == wanted)
}

/// The row a line naming `name` in `unit_id` lands on, if the list has one.
///
/// Trimmed and case-folded, because somebody typing the same word twice has not named
/// two things. The unit is part of it and deliberately so: `4 pint milk` and
/// `2 litre milk` really are two rows, being two amounts of two different things.
///
/// Outstanding before crossed-off, so a re-add lands on the row somebody can see.
pub fn alike<'a>(rows: &'a [Row], name: &str, unit_id: Option<i64>) -> Option<&'a Row> {
    let wanted = name.trim().to_lowercase();
    let matches = |row: &&Row| row.unit_id == unit_id && row.name.trim().to_lowercase() == wanted;

    rows.iter()
        .find(|r| !r.done && matches(r))
        .or_else(|| rows.iter().find(matches))
}

/// The whole of it: read the line, then decide.
/// `history` is the whole of what the list remembers; the entry for this line is found
/// here rather than by the caller, because which entry that is depends on reading the
/// line first — see [`recall`].
pub fn resolve(line: &str, units: &[Unit], rows: &[Row], history: &[Remembered]) -> Decision {
    let names: Vec<String> = units.iter().map(|u| u.name.clone()).collect();
    let standalone: Vec<String> = units
        .iter()
        .filter(|u| u.bare)
        .map(|u| u.name.clone())
        .collect();
    let parsed = quick_add::parse_with(line, &names, &standalone);
    // After the parse, and it has to be: the name is what the line turned out to mean.
    let remembered = recall(history, &parsed.name);
    let unit_id = unit_for(parsed.unit.as_deref(), remembered, units);
    let amount = amount_for(&parsed, remembered);

    match alike(rows, &parsed.name, unit_id) {
        Some(row) => Decision::Existing { uuid: row.uuid.clone(), put_back: row.done },
        None => Decision::New {
            // Named the way it will be stored. The server capitalises when it writes a
            // row; a device with no server has nobody to do that for it, so the same
            // keystrokes gave `milk` on a phone and `Milk` through a server. See
            // `crate::capitalise`.
            name: crate::capitalise(&parsed.name),
            amount,
            unit_id,
            tag_ids: remembered.map(|r| r.tag_ids.clone()).unwrap_or_default(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units() -> Vec<Unit> {
        vec![
            Unit { id: 1, name: "unit".into(), bare: false },
            Unit { id: 2, name: "kg".into(), bare: true },
            Unit { id: 3, name: "pint".into(), bare: true },
        ]
    }

    fn row(uuid: &str, name: &str, unit_id: Option<i64>, done: bool) -> Row {
        Row { uuid: uuid.into(), name: name.into(), unit_id, done }
    }

    #[test]
    fn an_unstated_unit_is_counted_rather_than_nothing() {
        assert_eq!(unit_for(None, None, &units()), Some(1));
    }

    #[test]
    fn the_line_outranks_the_memory() {
        let remembered = Remembered { name: "milk".into(), unit_id: Some(3), amount: None, tag_ids: vec![] };
        assert_eq!(unit_for(Some("kg"), Some(&remembered), &units()), Some(2));
    }

    #[test]
    fn the_memory_outranks_the_fallback() {
        let remembered = Remembered { name: "milk".into(), unit_id: Some(3), amount: None, tag_ids: vec![] };
        assert_eq!(unit_for(None, Some(&remembered), &units()), Some(3));
    }

    #[test]
    fn the_same_word_in_another_case_is_the_same_row() {
        let rows = vec![row("a", "Milk", Some(3), false)];
        assert_eq!(alike(&rows, "  milk ", Some(3)).map(|r| r.uuid.as_str()), Some("a"));
    }

    #[test]
    fn a_different_unit_is_a_different_row() {
        let rows = vec![row("a", "milk", Some(3), false)];
        assert!(alike(&rows, "milk", Some(2)).is_none());
    }

    #[test]
    fn an_outstanding_row_is_preferred_to_a_crossed_off_one() {
        // Both match. The one somebody can see is the one a re-add should land on.
        let rows = vec![row("done", "milk", Some(3), true), row("open", "milk", Some(3), false)];
        assert_eq!(alike(&rows, "milk", Some(3)).map(|r| r.uuid.as_str()), Some("open"));
    }

    #[test]
    fn adding_something_already_on_the_list_changes_nothing() {
        let rows = vec![row("a", "milk", Some(3), false)];
        assert_eq!(
            resolve("2 pint milk", &units(), &rows, &[]),
            Decision::Existing { uuid: "a".into(), put_back: false },
            "the amount moved, or a second row appeared"
        );
    }

    #[test]
    fn adding_something_crossed_off_brings_it_back() {
        // With the history, which is the real path: `milk` was last bought in pints,
        // so the bare word resolves to pints and finds the row.
        let rows = vec![row("a", "milk", Some(3), true)];
        let remembered = Remembered { name: "milk".into(), unit_id: Some(3), amount: None, tag_ids: vec![] };
        assert_eq!(
            resolve("milk", &units(), &rows, std::slice::from_ref(&remembered)),
            Decision::Existing { uuid: "a".into(), put_back: true }
        );
    }

    /// Worth pinning, because it looks wrong until you see why.
    ///
    /// With no memory of milk, a bare `milk` is one *unit* of milk, and the list's
    /// `2 pint milk` is something else — so this makes a second row. That is the same
    /// answer the server gives, and the history is what stops it happening in practice:
    /// anything bought in pints once is remembered in pints, so the bare word finds it.
    /// The two must agree even here, or a row appears on a phone and merges on a drain.
    #[test]
    fn a_bare_name_with_no_memory_does_not_match_a_measured_row() {
        let rows = vec![row("a", "milk", Some(3), false)];
        assert!(matches!(
            resolve("milk", &units(), &rows, &[]),
            Decision::New { unit_id: Some(1), .. }
        ));
    }

    #[test]
    fn a_new_line_arrives_filed_where_it_was_filed_last_time() {
        let remembered = Remembered { name: "milk".into(), unit_id: Some(3), amount: None, tag_ids: vec![7, 9] };
        assert_eq!(
            resolve("milk", &units(), &[], std::slice::from_ref(&remembered)),
            Decision::New {
                name: "Milk".into(),
                amount: 1.0,
                unit_id: Some(3),
                tag_ids: vec![7, 9],
            }
        );
    }

    #[test]
    fn a_bare_name_is_one_of_something_counted() {
        assert_eq!(
            resolve("bread", &units(), &[], &[]),
            Decision::New {
                name: "Bread".into(),
                amount: 1.0,
                unit_id: Some(1),
                tag_ids: vec![],
            }
        );
    }

    #[test]
    fn how_much_you_usually_buy_comes_back() {
        let remembered = Remembered { name: "apples".into(), unit_id: Some(2), amount: Some(2.0), tag_ids: vec![] };
        assert_eq!(
            resolve("apples", &units(), &[], std::slice::from_ref(&remembered)),
            Decision::New {
                name: "Apples".into(),
                amount: 2.0,
                unit_id: Some(2),
                tag_ids: vec![],
            }
        );
    }

    #[test]
    fn a_number_on_the_line_outranks_what_you_usually_buy() {
        let remembered = Remembered { name: "apples".into(), unit_id: Some(2), amount: Some(2.0), tag_ids: vec![] };
        assert!(matches!(
            resolve("1 kg apples", &units(), &[], std::slice::from_ref(&remembered)),
            Decision::New { amount, .. } if amount == 1.0
        ));
    }

    #[test]
    fn a_unit_with_no_number_is_still_a_unit() {
        assert_eq!(
            resolve("pint milk", &units(), &[], &[]),
            Decision::New {
                name: "Milk".into(),
                amount: 1.0,
                unit_id: Some(3),
                tag_ids: vec![],
            }
        );
    }

    #[test]
    fn a_line_with_a_quantity_still_finds_what_it_names() {
        // The bug: the caller looked the history up by what somebody *typed*, so this
        // went looking for a memory of "2 kg apples" and found nothing -- history
        // applied to bare words and to nothing else. The name is what the line turned
        // out to mean, so the lookup cannot happen before the parse.
        let history = vec![Remembered {
            name: "apples".into(),
            unit_id: Some(2),
            amount: Some(5.0),
            tag_ids: vec![7],
        }];

        let Decision::New { tag_ids, .. } = resolve("2 kg apples", &units(), &[], &history) else {
            panic!("expected a new row");
        };
        assert_eq!(tag_ids, vec![7], "the aisle it is always filed under was not applied");
    }

    #[test]
    fn a_name_is_recalled_however_it_was_spelled() {
        let history = vec![Remembered {
            name: "  Milk ".into(),
            unit_id: Some(3),
            amount: None,
            tag_ids: vec![],
        }];
        assert!(recall(&history, "MILK").is_some(), "the fold differs by end");
    }

    #[test]
    fn a_name_nothing_remembers_recalls_nothing() {
        let history = vec![Remembered {
            name: "milk".into(),
            unit_id: Some(3),
            amount: None,
            tag_ids: vec![],
        }];
        assert!(recall(&history, "bread").is_none());
    }
}
