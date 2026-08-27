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
