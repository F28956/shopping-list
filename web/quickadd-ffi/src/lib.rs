//! `2 kg apples`, for a caller that is not Rust.
//!
//! The parser is `parsing::quick_add` and stays there: it is the same forty-three cases
//! the server is tested against, and the whole point of this crate is that the phones
//! do not get a second opinion about what a typed line means. A shopping list that
//! reads `2 kg apples` one way on a phone and another way in a browser is a shopping
//! list that argues with itself.
//!
//! Deliberately tiny, and deliberately C. Every argument is a nul-terminated string
//! and every answer is one; there are no structs across the boundary, no ownership
//! rules to remember beyond "free what you were given", and nothing here that a
//! binding generator would do better.
//!
//! The answer comes back as JSON. It is three fields, and a JSON decoder is something
//! every caller already has — the alternative is an out-parameter struct whose layout
//! both sides have to agree on for ever.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Reads `line` against the unit names in `units_json`, and answers with JSON:
///
/// ```json
/// {"name": "apples", "amount": 2.0, "unit": "kg"}
/// ```
///
/// `unit` is absent when the line named none.
///
/// # Safety
///
/// Both arguments must be nul-terminated UTF-8. The answer is owned by the caller and
/// must be handed back to [`quickadd_free`] — not to `free`, because it was not
/// allocated by the caller's allocator.
///
/// Never returns null: a caller that cannot be told what went wrong would have to
/// guess, so bad input answers with the line as a name and an amount of one, which is
/// what the Rust parser does with anything it cannot read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quickadd_parse(
    line: *const c_char,
    units_json: *const c_char,
) -> *mut c_char {
    let line = unsafe { borrow(line) };
    let units: Vec<String> = unsafe { borrow(units_json) }
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();

    let line = line.unwrap_or("");

    // The documented promise two paragraphs up is that this never returns null and that
    // input it cannot read comes back as a name with an amount of one. A panic is input
    // it cannot read: `\u{212A} KELVIN SIGN` where somebody meant a `K` used to slice a
    // unit through the middle of a character, and an unwind across `extern "C"` is
    // undefined behaviour rather than a crash somebody could report. That particular
    // bug is fixed; the guarantee should not depend on my having found all of them.
    guarded(
        &serde_json::json!({ "name": line, "amount": 1.0, "unit": serde_json::Value::Null }),
        || {
            let parsed = parsing::quick_add::parse(line, &units);
            let answer = serde_json::json!({
                "name": parsed.name,
                "amount": parsed.amount,
                "unit": parsed.unit,
            });
            // `unwrap` on the CString: the only way it fails is an interior nul, and
            // this string was just built by a JSON serialiser that escapes them.
            CString::new(answer.to_string()).unwrap().into_raw()
        },
    )
}

/// Hands back what [`quickadd_parse`] returned.
///
/// # Safety
///
/// `answer` must be a pointer this library returned, and must not be used afterwards.
/// Null is ignored, so a caller does not have to check.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quickadd_free(answer: *mut c_char) {
    if !answer.is_null() {
        drop(unsafe { CString::from_raw(answer) });
    }
}

unsafe fn borrow<'a>(raw: *const c_char) -> Option<&'a str> {
    if raw.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(raw) }.to_str().ok()
}

/// The same parser, for the JVM.
///
/// Separate from the C entry points above rather than layered on them, because JNI is
/// not a C ABI: the symbol name is mangled from the Java class that declares the
/// method, and the strings are `jstring` handles rather than pointers. What the two
/// have in common is that neither decides anything -- both call `parsing::quick_add`
/// and hand back its answer.
///
/// Memory is the JVM's here, which is the one real difference for a reader: the
/// returned string is a Java object and the collector owns it, so there is no free
/// function to match this one.
#[cfg(target_os = "android")]
pub mod android {
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;

    /// The whole decision, as [`super::quickadd_resolve`] gives it.
    ///
    /// Here rather than left to Android because a client that only parses is a client
    /// that quietly does less: no merging of a line onto a row the list already has,
    /// no history, no unit written without a number. Those are rules, and this is how
    /// the JVM reaches them.
    #[unsafe(no_mangle)]
    pub extern "C" fn Java_com_cernauskas_shoppinglist_data_QuickAdd_resolve(
        mut env: JNIEnv,
        _class: JClass,
        input: JString,
    ) -> jstring {
        answer(&mut env, input, |raw| unsafe {
            let c = std::ffi::CString::new(raw).unwrap_or_default();
            super::quickadd_resolve(c.as_ptr())
        })
    }

    /// The remembered names worth offering, as [`super::quickadd_suggest`] gives them.
    #[unsafe(no_mangle)]
    pub extern "C" fn Java_com_cernauskas_shoppinglist_data_QuickAdd_suggest(
        mut env: JNIEnv,
        _class: JClass,
        input: JString,
    ) -> jstring {
        answer(&mut env, input, |raw| unsafe {
            let c = std::ffi::CString::new(raw).unwrap_or_default();
            super::quickadd_suggest(c.as_ptr())
        })
    }

    /// Shared plumbing: a Java string in, one of the C entry points, a Java string out.
    ///
    /// The C function owns what it returns, so it is copied into a `jstring` and then
    /// freed here -- the JVM's collector cannot know about a `CString`.
    fn answer(
        env: &mut JNIEnv,
        input: JString,
        call: impl FnOnce(&str) -> *mut std::ffi::c_char,
    ) -> jstring {
        let raw: String = env.get_string(&input).map(Into::into).unwrap_or_default();
        let produced = call(&raw);
        if produced.is_null() {
            return std::ptr::null_mut();
        }

        let text = unsafe { std::ffi::CStr::from_ptr(produced) }
            .to_string_lossy()
            .into_owned();
        unsafe { super::quickadd_free(produced) };

        match env.new_string(text) {
            Ok(made) => made.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    /// Answers with the same JSON the C entry point does.
    ///
    /// The name is not a name: JNI resolves
    /// `Java_com_cernauskas_shoppinglist_data_QuickAdd_parse` from the package, class
    /// and method it is declared in. Renaming or moving that Kotlin object without
    /// renaming this breaks the link at run time, not at build time.
    #[unsafe(no_mangle)]
    pub extern "C" fn Java_com_cernauskas_shoppinglist_data_QuickAdd_parse(
        mut env: JNIEnv,
        _class: JClass,
        line: JString,
        units_json: JString,
    ) -> jstring {
        let line: String = env.get_string(&line).map(Into::into).unwrap_or_default();
        let units_json: String = env
            .get_string(&units_json)
            .map(Into::into)
            .unwrap_or_default();
        let units: Vec<String> = serde_json::from_str(&units_json).unwrap_or_default();

        let parsed = parsing::quick_add::parse(&line, &units);
        let answer = serde_json::json!({
            "name": parsed.name,
            "amount": parsed.amount,
            "unit": parsed.unit,
        });

        // A failure here means the JVM could not allocate a string, which it will
        // already be throwing about. Null is what JNI expects to be returned while an
        // exception is pending.
        match env.new_string(answer.to_string()) {
            Ok(made) => made.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Ranks remembered names for something part-typed.
///
/// The clients used to have no answer here at all: suggestions came from the server,
/// so a device with none offered nothing, and the history that makes `milk` arrive in
/// pints under dairy simply did not exist. The store is the device's own -- it is that
/// person's shopping and there is nowhere else for it to live -- but **the policy is
/// not**: which of "milk" and "milk chocolate" comes first when you have typed `mil`
/// is a judgement, and a judgement made twice is a judgement that will differ.
///
/// `input` is the query and the candidates together:
///
/// ```json
/// {"query": "mil", "now": 1756300000,
///  "candidates": [{"name": "milk", "uses": 12, "last_used_at": 1756200000}]}
/// ```
///
/// The answer is the names that matched, best first: `{"names": ["milk"]}`.
/// Matching is `fuzzy` and ordering is `history_rank`, both the server's own.
///
/// # Safety
///
/// As [`quickadd_parse`]: nul-terminated UTF-8 in, a string for [`quickadd_free`] out.
/// Never returns null; input it cannot read answers with no names rather than
/// pretending the history is empty in some more interesting way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quickadd_suggest(input: *const c_char) -> *mut c_char {
    let parsed: Option<Query> =
        unsafe { borrow(input) }.and_then(|raw| serde_json::from_str(raw).ok());

    guarded(&serde_json::json!({ "names": [] }), move || {
        let names = match parsed {
            None => Vec::new(),
            // The whole policy is `parsing::suggest` -- which names are candidates, and in
            // what order. It used to be half here: this filtered by fuzzy score and then
            // ordered by how often a thing is bought, while the server ordered by how well
            // it matched and broke ties on use. `mil` offered `milk` on one and `milk
            // chocolate` on the other.
            Some(query) => parsing::suggest::offer(
                &query.query,
                query
                    .candidates
                    .into_iter()
                    .map(|c| parsing::suggest::Remembered {
                        name: c.name,
                        uses: c.uses,
                        last_used_at: c.last_used_at,
                    })
                    .collect(),
                query.now,
            ),
        };

        CString::new(serde_json::json!({ "names": names }).to_string())
            .unwrap()
            .into_raw()
    })
}

/// Runs `work`, answering with `fallback` if it panics.
///
/// Each of these entry points documents that it never returns null and that input it
/// cannot read gets a harmless answer rather than a special case. A panic is the same
/// situation arrived at by a different road -- and unlike the server, which has axum's
/// catch-panic layer in front of it, there is nothing between this and somebody's app.
/// An unwind across `extern "C"` is undefined behaviour.
fn guarded(fallback: &serde_json::Value, work: impl FnOnce() -> *mut c_char) -> *mut c_char {
    catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|_| CString::new(fallback.to_string()).unwrap().into_raw())
}

#[derive(serde::Deserialize)]
struct Query {
    query: String,
    /// Unix seconds. Passed in rather than read here: this crate has no clock, and a
    /// caller that wants deterministic answers in a test should be able to have them.
    now: i64,
    candidates: Vec<Remembered>,
}

#[derive(serde::Deserialize)]
struct Remembered {
    name: String,
    uses: i64,
    last_used_at: i64,
}

/// What a typed line should do to a list.
///
/// The whole decision, not just the parse: which unit it lands in, whether the list
/// already has that row, and whether a crossed-off one comes back. See
/// [`parsing::add`] for the rules and for why they are not written out again here.
///
/// This is the entry point the clients should reach for when somebody types a line.
/// [`quickadd_parse`] is the smaller question -- what do these words mean -- and is
/// still right for a caller that only wants that.
///
/// ```json
/// {"line": "2 pint milk",
///  "units": [{"id": 3, "name": "pint"}],
///  "rows": [{"uuid": "abc", "name": "milk", "unit_id": 3, "done": true}],
///  "history": [{"name": "milk", "unit_id": 3, "amount": 2.0, "tag_ids": [7]}]}
/// ```
///
/// answers with one of:
///
/// ```json
/// {"existing": {"uuid": "abc", "put_back": true}}
/// {"new": {"name": "milk", "amount": 2.0, "unit_id": 3, "tag_ids": [7]}}
/// ```
///
/// # Safety
///
/// As [`quickadd_parse`]: nul-terminated UTF-8 in, a string for [`quickadd_free`] out.
/// Never returns null. Input it cannot read answers with a `new` row named by the line
/// as given, which is what an unreadable line means everywhere else here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn quickadd_resolve(input: *const c_char) -> *mut c_char {
    let raw = unsafe { borrow(input) }.unwrap_or("");
    let asked: Option<Asked> = serde_json::from_str(raw).ok();

    // The same fallback the unreadable-input branch below uses, so a panic and a line
    // this cannot make sense of are one answer rather than two.
    guarded(
        &serde_json::json!({
            "new": { "name": "", "amount": 1.0, "unit_id": null, "tag_ids": [] }
        }),
        move || {
            let answer = match asked {
                None => serde_json::json!({
                    "new": { "name": "", "amount": 1.0, "unit_id": null, "tag_ids": [] }
                }),
                Some(asked) => {
                    let units: Vec<parsing::add::Unit> = asked
                        .units
                        .into_iter()
                        .map(|u| parsing::add::Unit {
                            id: u.id,
                            name: u.name,
                            bare: u.bare,
                        })
                        .collect();
                    let rows: Vec<parsing::add::Row> = asked
                        .rows
                        .into_iter()
                        .map(|r| parsing::add::Row {
                            uuid: r.uuid,
                            name: r.name,
                            unit_id: r.unit_id,
                            done: r.done,
                        })
                        .collect();
                    // The whole memory, not one entry. Which entry applies depends on what
                    // the line turns out to name, so the caller cannot know it in advance --
                    // see `parsing::add::recall`.
                    let history: Vec<parsing::add::Remembered> = asked
                        .history
                        .into_iter()
                        .map(|r| parsing::add::Remembered {
                            name: r.name,
                            unit_id: r.unit_id,
                            amount: r.amount,
                            tag_ids: r.tag_ids,
                        })
                        .collect();

                    match parsing::add::resolve(&asked.line, &units, &rows, &history) {
                        parsing::add::Decision::Existing { uuid, put_back } => {
                            serde_json::json!({ "existing": { "uuid": uuid, "put_back": put_back } })
                        }
                        parsing::add::Decision::New {
                            name,
                            amount,
                            unit_id,
                            tag_ids,
                        } => {
                            serde_json::json!({
                                "new": {
                                    "name": name,
                                    "amount": amount,
                                    "unit_id": unit_id,
                                    "tag_ids": tag_ids,
                                }
                            })
                        }
                    }
                }
            };

            CString::new(answer.to_string()).unwrap().into_raw()
        },
    )
}

#[derive(serde::Deserialize)]
struct Asked {
    line: String,
    units: Vec<AskedUnit>,
    #[serde(default)]
    rows: Vec<AskedRow>,
    /// Everything this list remembers. Empty is a list that remembers nothing, which
    /// is an ordinary state and not a missing field.
    #[serde(default)]
    history: Vec<AskedRemembered>,
}

#[derive(serde::Deserialize)]
struct AskedUnit {
    id: i64,
    name: String,
    /// Whether it may be written with no number in front of it -- `pint milk`.
    ///
    /// **Deliberately not `#[serde(default)]`.** It was, so that a caller which had
    /// not learned about the field would keep the old behaviour -- and the effect was
    /// that a caller which *forgot* it got the old behaviour too, silently, which is
    /// how `pint milk` came to mean one unit of "pint milk" on a phone. There is no
    /// older caller: this library is compiled from the same tree as the apps that
    /// link it. A field left out is a mistake, and it should say so.
    bare: bool,
}

#[derive(serde::Deserialize)]
struct AskedRow {
    uuid: String,
    name: String,
    unit_id: Option<i64>,
    done: bool,
}

#[derive(serde::Deserialize)]
struct AskedRemembered {
    name: String,
    unit_id: Option<i64>,
    #[serde(default)]
    amount: Option<f64>,
    #[serde(default)]
    tag_ids: Vec<i64>,
}

#[cfg(test)]
mod a_panic_never_crosses_the_boundary {
    use super::*;

    /// Proof that the net is real, rather than that it compiles.
    ///
    /// `guarded` is what every entry point returns through, so panicking inside it is
    /// the same situation as panicking inside the parser -- without needing a parser
    /// bug on hand to demonstrate it, which would only work until the bug was fixed.
    #[test]
    fn a_panic_becomes_the_documented_fallback() {
        let fallback = serde_json::json!({ "name": "2 kg apples", "amount": 1.0 });
        let answer = guarded(&fallback, || panic!("something went wrong deep inside"));

        assert!(!answer.is_null(), "the promise is that this is never null");
        let read = unsafe { CStr::from_ptr(answer) }.to_str().unwrap().to_string();
        unsafe { quickadd_free(answer) };

        let parsed: serde_json::Value = serde_json::from_str(&read).unwrap();
        assert_eq!(parsed["name"], "2 kg apples", "the line was not handed back");
        assert_eq!(parsed["amount"], 1.0);
    }

    /// And the ordinary path is untouched: the guard returns what the work returned,
    /// not a copy of it, so nothing is leaked or double-freed.
    #[test]
    fn work_that_does_not_panic_answers_for_itself() {
        let line = std::ffi::CString::new("2 kg apples").unwrap();
        let units = std::ffi::CString::new(r#"["kg"]"#).unwrap();

        let answer = unsafe { quickadd_parse(line.as_ptr(), units.as_ptr()) };
        let read = unsafe { CStr::from_ptr(answer) }.to_str().unwrap().to_string();
        unsafe { quickadd_free(answer) };

        let parsed: serde_json::Value = serde_json::from_str(&read).unwrap();
        assert_eq!(parsed["name"], "apples");
        assert_eq!(parsed["unit"], "kg");
        assert_eq!(parsed["amount"], 2.0);
    }

    /// The line that used to abort the host app, through the real entry point.
    #[test]
    fn a_kelvin_sign_answers_rather_than_aborting() {
        let line = std::ffi::CString::new("2 \u{212A}g milk").unwrap();
        let units = std::ffi::CString::new(r#"["g","kg"]"#).unwrap();

        let answer = unsafe { quickadd_parse(line.as_ptr(), units.as_ptr()) };
        assert!(!answer.is_null());
        let read = unsafe { CStr::from_ptr(answer) }.to_str().unwrap().to_string();
        unsafe { quickadd_free(answer) };

        let parsed: serde_json::Value = serde_json::from_str(&read).unwrap();
        assert!(
            parsed["name"].as_str().unwrap().contains("milk"),
            "the line was lost: {read}"
        );
    }
}
