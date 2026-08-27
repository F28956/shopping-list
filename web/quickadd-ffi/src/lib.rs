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

    let parsed = parsing::quick_add::parse(line.unwrap_or(""), &units);

    let answer = serde_json::json!({
        "name": parsed.name,
        "amount": parsed.amount,
        "unit": parsed.unit,
    });

    // `unwrap` on the CString: the only way it fails is an interior nul, and this
    // string was just built by a JSON serialiser that escapes them.
    CString::new(answer.to_string()).unwrap().into_raw()
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
    let parsed: Option<Query> = unsafe { borrow(input) }.and_then(|raw| serde_json::from_str(raw).ok());

    let names = match parsed {
        None => Vec::new(),
        Some(query) => {
            let matched: Vec<parsing::history_rank::Candidate<String>> = query
                .candidates
                .into_iter()
                // Scored, then discarded: `rank` decides the order and a fuzzy score
                // is only about whether it belongs in the running at all. Sorting by
                // it instead would put a close spelling above something bought weekly.
                .filter(|c| parsing::fuzzy::score(&query.query, &c.name).is_some())
                .map(|c| parsing::history_rank::Candidate {
                    value: c.name,
                    uses: c.uses,
                    last_used_at: c.last_used_at,
                })
                .collect();

            parsing::history_rank::rank(matched, query.now)
        }
    };

    CString::new(serde_json::json!({ "names": names }).to_string())
        .unwrap()
        .into_raw()
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
