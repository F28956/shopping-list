//! The device's server, for a caller that is not Rust.
//!
//! The same shape as `quickadd-ffi`, and for the same reasons: every argument is a
//! nul-terminated string, every answer is one, no structs cross the boundary, and the
//! only ownership rule is "free what you were given". A binding generator would not do
//! this better, and it would be another thing to install.
//!
//! ## Every answer is an envelope
//!
//! ```json
//! {"ok": [ … ]}
//! {"error": "That list is gone."}
//! ```
//!
//! Rather than a null return and a separate "what went wrong" call. A caller that has
//! to make two calls to learn one thing will eventually make only the first, and the
//! two cannot be made atomically across a boundary that several threads share.
//!
//! The rows inside `ok` are `domain`'s own types, serialised by `domain`'s own derives.
//! That is deliberate: the wire between a device and its own database is the same wire
//! the server answers on, so a client that can read one can read the other. It is also
//! what makes the two modes one thing rather than two -- see the crate documentation.
//!
//! ## Threads
//!
//! A `Local` may be used from several threads; sqlx's pool is built for it. A `Watcher`
//! may not -- `next` takes it exclusively -- which is why stopping goes through a
//! separate `Stopper` that any thread may hold.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use crate::{Change, Local, Stopper, Watcher};

// ------------------------------------------------------------------ the database

/// Opens the database at `path`, migrating it and finding this device's person.
///
/// # Safety
///
/// `path` must be nul-terminated UTF-8. The answer is a handle owned by the caller and
/// must be given back to [`embedded_close`]. Null means the database could not be
/// opened -- a disk that will not cooperate, or a path that is not writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_open(path: *const c_char) -> *mut Local {
    let Some(path) = (unsafe { borrow(path) }) else {
        return std::ptr::null_mut();
    };

    // Opening runs the migrations, which is the most eventful thing this library does
    // and the likeliest place to trip an assertion in `domain`. Null is the answer the
    // caller already handles for a database that will not open.
    guarded(std::ptr::null_mut(), || {
        match Local::open(&PathBuf::from(path)) {
            Ok(local) => Box::into_raw(Box::new(local)),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Closes a database opened by [`embedded_open`].
///
/// # Safety
///
/// `handle` must have come from [`embedded_open`] and must not be used again. Null is
/// accepted and does nothing, so a caller tearing down a half-built object does not
/// have to check.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_close(handle: *mut Local) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle) });
}

/// This device's person, as a number. Zero when the handle is null.
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_me(handle: *const Local) -> i64 {
    let Some(local) = (unsafe { handle.as_ref() }) else {
        return 0;
    };
    guarded(0, || local.me())
}

// ------------------------------------------------------------------ lists

/// `{"ok": [ … lists … ]}`
///
/// # Safety
///
/// `handle` must be live. The answer must be given back to [`embedded_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_lists(handle: *const Local) -> *mut c_char {
    unsafe { answering(handle, |local| local.lists()) }
}

/// # Safety
///
/// `handle` must be live and `name` nul-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_make_list(
    handle: *const Local,
    name: *const c_char,
) -> *mut c_char {
    let name = unsafe { borrow(name) }.unwrap_or_default().to_string();
    unsafe { answering(handle, move |local| local.make_list(&name)) }
}

/// # Safety
///
/// `handle` must be live and `name` nul-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_rename_list(
    handle: *const Local,
    id: i64,
    name: *const c_char,
) -> *mut c_char {
    let name = unsafe { borrow(name) }.unwrap_or_default().to_string();
    unsafe { answering(handle, move |local| local.rename_list(id, &name)) }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_delete_list(handle: *const Local, id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.delete_list(id)) }
}

// ------------------------------------------------------------------ what is on one

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_items(handle: *const Local, list_id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.items(list_id)) }
}

/// Adds what somebody typed, read the way the server reads it.
///
/// `uuid` may be null, for a caller that has not already drawn the row.
///
/// # Safety
///
/// `handle` must be live; `line` and `uuid` nul-terminated UTF-8 or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_add(
    handle: *const Local,
    list_id: i64,
    line: *const c_char,
    uuid: *const c_char,
) -> *mut c_char {
    let line = unsafe { borrow(line) }.unwrap_or_default().to_string();
    let uuid = unsafe { borrow(uuid) }.map(str::to_string);
    unsafe { answering(handle, move |local| local.add(list_id, &line, uuid)) }
}

/// # Safety
/// Crosses something off, or puts it back.
///
/// `at_seconds` is when the tick happened, or zero for now -- a watch's tick may be an
/// hour old by the time the two devices are in range, and the ordering rules run on when
/// somebody decided rather than when the news arrived.
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_set_done(
    handle: *const Local,
    item_id: i64,
    done: bool,
    at_seconds: i64,
) -> *mut c_char {
    let at = if at_seconds == 0 {
        None
    } else {
        Some(at_seconds)
    };
    unsafe { answering(handle, move |local| local.set_done(item_id, done, at)) }
}

/// `unit_id` of zero means none, because C has no optional and the units are counted
/// from one.
///
/// # Safety
///
/// `handle` must be live and `name` nul-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_update_item(
    handle: *const Local,
    item_id: i64,
    name: *const c_char,
    amount: f64,
    unit_id: i64,
) -> *mut c_char {
    let name = unsafe { borrow(name) }.unwrap_or_default().to_string();
    let unit = if unit_id == 0 { None } else { Some(unit_id) };
    unsafe {
        answering(handle, move |local| {
            local.update(item_id, &name, amount, unit)
        })
    }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_delete_item(handle: *const Local, item_id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.delete_item(item_id)) }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_clear_done(handle: *const Local, list_id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.clear_done(list_id)) }
}

// ------------------------------------------------------------------ taking over

/// Brings a device's old cache across. See `Local::import`.
///
/// Answers `{"ok": 4}` with the number of items brought.
///
/// # Safety
///
/// `handle` must be live and `everything_json` nul-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_import(
    handle: *const Local,
    everything_json: *const c_char,
) -> *mut c_char {
    let Some(raw) = (unsafe { borrow(everything_json) }) else {
        return owned(&serde_json::json!({ "error": "nothing to import" }));
    };
    let everything: crate::Incoming = match serde_json::from_str(raw) {
        Ok(read) => read,
        Err(why) => return owned(&serde_json::json!({ "error": why.to_string() })),
    };

    unsafe { answering(handle, move |local| local.import(&everything)) }
}

// --------------------------------------------- what things are called and grouped

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_units(handle: *const Local) -> *mut c_char {
    unsafe { answering(handle, |local| local.units()) }
}

/// The categories in this list's order.
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_tags(handle: *const Local, list_id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.tags(list_id)) }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_tags_on(handle: *const Local, item_id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.tags_on(item_id)) }
}

/// The order, as a JSON array of tag ids: `[5, 3, 9]`.
///
/// An array rather than a repeated call, because the order is one fact about the list
/// and applying it row by row would leave it half-applied if anything failed.
///
/// # Safety
///
/// `handle` must be live and `tag_ids_json` nul-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_set_tag_order(
    handle: *const Local,
    list_id: i64,
    tag_ids_json: *const c_char,
) -> *mut c_char {
    let ids: Vec<i64> = unsafe { borrow(tag_ids_json) }
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    unsafe { answering(handle, move |local| local.set_tag_order(list_id, &ids)) }
}

/// `emoji` may be null.
///
/// # Safety
///
/// `handle` must be live; `name` and `emoji` nul-terminated UTF-8 or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_create_tag(
    handle: *const Local,
    name: *const c_char,
    emoji: *const c_char,
) -> *mut c_char {
    let name = unsafe { borrow(name) }.unwrap_or_default().to_string();
    let emoji = unsafe { borrow(emoji) }.map(str::to_string);
    unsafe { answering(handle, move |local| local.create_tag(&name, emoji)) }
}

/// # Safety
///
/// `handle` must be live; `name` and `emoji` nul-terminated UTF-8 or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_update_tag(
    handle: *const Local,
    id: i64,
    name: *const c_char,
    emoji: *const c_char,
) -> *mut c_char {
    let name = unsafe { borrow(name) }.unwrap_or_default().to_string();
    let emoji = unsafe { borrow(emoji) }.map(str::to_string);
    unsafe { answering(handle, move |local| local.update_tag(id, &name, emoji)) }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_delete_tag(handle: *const Local, id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.delete_tag(id)) }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_attach_tag(
    handle: *const Local,
    item_id: i64,
    tag_id: i64,
) -> *mut c_char {
    unsafe { answering(handle, |local| local.attach_tag(item_id, tag_id)) }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_detach_tag(
    handle: *const Local,
    item_id: i64,
    tag_id: i64,
) -> *mut c_char {
    unsafe { answering(handle, |local| local.detach_tag(item_id, tag_id)) }
}

// ------------------------------------------------------------------ what is bought

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_history(handle: *const Local, list_id: i64) -> *mut c_char {
    unsafe { answering(handle, |local| local.history(list_id)) }
}

/// `query` may be null or empty, which asks for the most recent rather than a match.
///
/// # Safety
///
/// `handle` must be live and `query` nul-terminated UTF-8 or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_suggestions(
    handle: *const Local,
    list_id: i64,
    query: *const c_char,
) -> *mut c_char {
    let query = unsafe { borrow(query) }.unwrap_or_default().to_string();
    unsafe { answering(handle, move |local| local.suggestions(list_id, &query)) }
}

// ------------------------------------------------------------------ being told

/// Starts watching one list. Give the answer back to [`embedded_watcher_free`].
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_watch_list(handle: *const Local, list_id: i64) -> *mut Watcher {
    let Some(local) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    // `watching_runtime` builds a runtime with `expect`, so this one can genuinely
    // panic rather than only theoretically.
    guarded(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(local.watch_list(list_id)))
    })
}

/// Starts watching which lists this person can see.
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_watch_lists(handle: *const Local) -> *mut Watcher {
    let Some(local) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    guarded(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(local.watch_lists()))
    })
}

/// **Blocks** until there is something to re-read, and answers with what:
///
/// ```json
/// {"list": 4}
/// {"lists": true}
/// ```
///
/// Null means the watch has ended and the calling thread should finish. It never means
/// "nothing happened", so a caller that loops on it does not spin.
///
/// Call this on a thread of your own. It will sit there for as long as nothing changes,
/// which on a shopping list is most of the time.
///
/// # Safety
///
/// `watcher` must be live and must not be used from another thread at the same time.
/// To end the wait, hold a [`embedded_watcher_stopper`] and call it from wherever the
/// screen is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_next_change(watcher: *mut Watcher) -> *mut c_char {
    let Some(watcher) = (unsafe { watcher.as_mut() }) else {
        return std::ptr::null_mut();
    };

    // Null is "the watch has ended", which is what a caller does with a watcher that
    // has died anyway.
    guarded(std::ptr::null_mut(), || match watcher.wait() {
        Some(Change::List(id)) => owned(&serde_json::json!({ "list": id })),
        Some(Change::Lists) => owned(&serde_json::json!({ "lists": true })),
        None => std::ptr::null_mut(),
    })
}

/// A handle that ends this watch, usable from any thread. Give it back to
/// [`embedded_stopper_free`].
///
/// # Safety
///
/// `watcher` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_watcher_stopper(watcher: *const Watcher) -> *mut Stopper {
    match unsafe { watcher.as_ref() } {
        Some(watcher) => Box::into_raw(Box::new(watcher.stopper())),
        None => std::ptr::null_mut(),
    }
}

/// Ends the watch. The thread parked in [`embedded_next_change`] returns null.
///
/// # Safety
///
/// `stopper` must be live. Safe to call from any thread, and safe to call twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_stop(stopper: *const Stopper) {
    if let Some(stopper) = unsafe { stopper.as_ref() } {
        stopper.stop();
    }
}

/// # Safety
///
/// `stopper` must have come from [`embedded_watcher_stopper`] and must not be used
/// again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_stopper_free(stopper: *mut Stopper) {
    if stopper.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(stopper) });
}

/// # Safety
///
/// `watcher` must have come from one of the `embedded_watch_*` calls and must not be
/// used again. **Stop it first**: freeing a watcher another thread is parked in is a
/// use-after-free, and no amount of care here can prevent that.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_watcher_free(watcher: *mut Watcher) {
    if watcher.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(watcher) });
}

// ------------------------------------------------------------------ strings

/// Hands back any string this module returned.
///
/// # Safety
///
/// The pointer must have come from this module and must not be used again. Not `free`:
/// it was not allocated by the caller's allocator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_free(answer: *mut c_char) {
    if answer.is_null() {
        return;
    }
    drop(unsafe { CString::from_raw(answer) });
}

// ------------------------------------------------------------------ the plumbing

/// Runs something against the database and wraps the outcome in the envelope.
///
/// One place, so every call answers in the same shape and a caller writes one decoder.
///
/// # Safety
///
/// `handle` must be null or a live `Local` from [`embedded_open`].
unsafe fn answering<T: serde::Serialize>(
    handle: *const Local,
    work: impl FnOnce(&Local) -> Result<T, crate::Error>,
) -> *mut c_char {
    owned(&enveloped(unsafe { handle.as_ref() }, work))
}

/// The envelope itself, before it is turned into whatever the caller's language wants.
///
/// Split out from [`answering`] so that JNI can share it. Android does not get a
/// `char *` -- every string it sees is a `jstring` minted from the JVM -- but it must
/// get the *same* envelope, or the two platforms would be reading two different
/// protocols against one database.
pub(crate) fn enveloped<T: serde::Serialize>(
    local: Option<&Local>,
    work: impl FnOnce(&Local) -> Result<T, crate::Error>,
) -> serde_json::Value {
    let Some(local) = local else {
        return serde_json::json!({ "error": "no database" });
    };

    // The envelope this module promises includes the case nobody plans for. A panic
    // crossing `extern "C"` is undefined behaviour, not a tidy crash, and every one of
    // these calls happens inside somebody's shopping list app rather than behind an
    // HTTP handler with a catch-panic layer in front of it. Unwinding is kept on for
    // exactly this reason -- see the workspace `[profile.release]`.
    //
    // `AssertUnwindSafe` because the answer is an error and the handle is not used
    // again on this path: nothing observes a half-updated value. The database is
    // sqlx's to keep consistent, and it does that with transactions rather than with
    // Rust's unwind safety.
    match catch_unwind(AssertUnwindSafe(|| work(local))) {
        Ok(Ok(answer)) => serde_json::json!({ "ok": answer }),
        Ok(Err(refusal)) => serde_json::json!({ "error": refusal.to_string() }),
        Err(panic) => serde_json::json!({ "error": describe(&panic) }),
    }
}

/// Whatever a panic was carrying, as something a person could be shown.
pub(crate) fn describe(panic: &Box<dyn std::any::Any + Send>) -> String {
    let said = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("something went wrong inside the database");
    format!("internal error: {said}")
}

/// Runs `work`, answering with `fallback` if it panics.
///
/// For the handful of entry points that do not go through [`answering`] because they
/// return a handle or a number rather than an envelope. There is nowhere to put a
/// message in those, so the fallback is the same "it did not work" the caller already
/// has to handle -- a null handle, or a zero.
pub(crate) fn guarded<T>(fallback: T, work: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or(fallback)
}

/// A borrowed `&str` from C, or `None` for null and for bytes that are not UTF-8.
///
/// # Safety
///
/// `raw` must be null or nul-terminated.
unsafe fn borrow<'a>(raw: *const c_char) -> Option<&'a str> {
    if raw.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(raw) }.to_str().ok()
}

/// JSON, as a string the caller owns.
fn owned(value: &serde_json::Value) -> *mut c_char {
    // `unwrap` on the CString: it fails only on an interior nul, and a JSON serialiser
    // escapes those.
    CString::new(value.to_string()).unwrap().into_raw()
}

#[cfg(test)]
mod a_panic_never_crosses_the_boundary {
    use super::*;

    /// Proof that the net is real rather than that it compiles.
    ///
    /// `answering` is what every database call returns through, so a panic inside the
    /// work is the same situation as a panic inside `domain` -- without needing a bug
    /// in `domain` on hand to demonstrate it.
    ///
    /// The envelope is the point. This module's contract is that every answer is
    /// `{"ok": …}` or `{"error": …}`, and until now a panic was neither: it unwound
    /// across `extern "C"`, which is undefined behaviour, in an app with no
    /// catch-panic layer anywhere near it.
    #[test]
    fn a_panic_inside_the_work_becomes_an_error_envelope() {
        let dir = std::env::temp_dir().join(format!("panic-probe-{}.sqlite", std::process::id()));
        let local = Local::open(&dir).expect("a fresh database");
        let handle = Box::into_raw(Box::new(local));

        let answer = unsafe {
            answering(handle, |_| -> Result<(), crate::Error> {
                panic!("something went wrong deep inside")
            })
        };

        assert!(!answer.is_null());
        let read = unsafe { CStr::from_ptr(answer) }.to_str().unwrap().to_string();
        unsafe { embedded_free(answer) };
        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&dir);

        let parsed: serde_json::Value = serde_json::from_str(&read).unwrap();
        assert!(parsed.get("ok").is_none(), "a panic was reported as success");
        let said = parsed["error"].as_str().expect("no error in the envelope");
        assert!(
            said.contains("something went wrong deep inside"),
            "what went wrong was not carried out: {said}"
        );
    }

    /// And `guarded`, which the entry points that return a handle rather than an
    /// envelope go through -- there is nowhere to put a message in a pointer.
    #[test]
    fn a_panic_where_there_is_no_envelope_answers_with_the_fallback() {
        let answer: *mut c_char = guarded(std::ptr::null_mut(), || panic!("no runtime"));
        assert!(answer.is_null(), "a panic produced a pointer to something");

        let fine: i64 = guarded(0, || 7);
        assert_eq!(fine, 7, "the ordinary path stopped answering for itself");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the boundary the way a client does: raw pointers, C strings, JSON back.
    ///
    /// The Rust API above it is already tested. What is tested here is the boundary
    /// itself -- that the envelope is the shape the documentation claims, that a
    /// refusal comes back as one rather than as a crash, and that a watcher started
    /// and stopped through C behaves as it does in Rust.
    fn c(text: &str) -> CString {
        CString::new(text).unwrap()
    }

    /// Reads an answer and frees it, which is what a caller must do.
    unsafe fn read(answer: *mut c_char) -> serde_json::Value {
        assert!(
            !answer.is_null(),
            "a call answered null where an envelope was promised"
        );
        let json: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(answer) }.to_str().unwrap()).unwrap();
        unsafe { embedded_free(answer) };
        json
    }

    fn scratch() -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "embedded-ffi-{}-{:?}.sqlite",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_list_and_its_items_across_the_boundary() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };
        assert!(!handle.is_null(), "the database did not open");
        assert!(
            unsafe { embedded_me(handle) } > 0,
            "no person for the device"
        );

        let made = unsafe { read(embedded_make_list(handle, c("Household").as_ptr())) };
        let list_id = made["ok"]["id"].as_i64().expect("a list with an id");
        assert_eq!(made["ok"]["name"], "Household");

        // Read the server's way: the whole line, resolved by the server's own reader.
        let added = unsafe {
            read(embedded_add(
                handle,
                list_id,
                c("2 kg apples").as_ptr(),
                std::ptr::null(),
            ))
        };
        // `Apples`, capitalised, and `2` -- because the *server* read the line. That
        // capital is the point of the whole exercise: it is `parsing::capitalise`
        // running here, on the device, rather than each client having its own opinion
        // about what a typed line becomes. This assertion was written lowercase and
        // was wrong, which is a small demonstration of the thing being fixed.
        assert_eq!(added["ok"]["name"], "Apples");
        assert_eq!(added["ok"]["amount"], 2.0);
        let item_id = added["ok"]["id"].as_i64().unwrap();

        let listed = unsafe { read(embedded_items(handle, list_id)) };
        assert_eq!(listed["ok"].as_array().unwrap().len(), 1);

        let ticked = unsafe { read(embedded_set_done(handle, item_id, true, 0)) };
        assert!(
            !ticked["ok"]["done_at"].is_null(),
            "ticking off did not stick"
        );

        let cleared = unsafe { read(embedded_clear_done(handle, list_id)) };
        assert_eq!(cleared["ok"], 1);
        assert!(
            unsafe { read(embedded_items(handle, list_id)) }["ok"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }

    /// The claim the whole crate rests on: a device answers the shape a server does.
    ///
    /// Not "close enough". A client that has to know which of the two it is talking to
    /// in order to find a field is a client with the fork still in it, and the fork is
    /// the thing being removed.
    #[test]
    fn the_wire_is_the_servers_wire() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };
        let list_id =
            unsafe { read(embedded_make_list(handle, c("Household").as_ptr())) }["ok"]["id"]
                .as_i64()
                .unwrap();
        let item_id = unsafe {
            read(embedded_add(
                handle,
                list_id,
                c("milk").as_ptr(),
                std::ptr::null(),
            ))
        }["ok"]["id"]
            .as_i64()
            .unwrap();
        let tag_id = unsafe { read(embedded_tags(handle, list_id)) }["ok"][0]["id"]
            .as_i64()
            .unwrap();
        unsafe { read(embedded_attach_tag(handle, item_id, tag_id)) };

        // A list carries `role`, which the API adds and the bare row does not have.
        // Without it the Swift decoder falls back to viewer and the app hides renaming
        // and deleting on every list the device owns.
        let listed = unsafe { read(embedded_lists(handle)) };
        assert_eq!(
            listed["ok"][0]["role"], "owner",
            "no role on a list: {listed}"
        );
        assert!(
            listed["ok"][0]["uuid"].as_str().is_some(),
            "no uuid on a list"
        );

        // An item carries `tag_ids`, which the API joins in. Without it every row is
        // filed under nothing and the shop is walked in no order at all.
        let items = unsafe { read(embedded_items(handle, list_id)) };
        assert_eq!(
            items["ok"][0]["tag_ids"].as_array().map(|t| t.len()),
            Some(1),
            "no tag_ids on an item: {items}"
        );
        assert!(
            items["ok"][0]["done_at"].is_null(),
            "a fresh item was already done"
        );

        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }

    /// The whole surface a list screen needs, in one pass, because a conformer that
    /// answers nine questions and crashes on the tenth is worse than one that does not
    /// compile. Every call here is one the Swift `Backend` protocol declares.
    #[test]
    fn everything_a_list_screen_asks_for() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };

        let list_id =
            unsafe { read(embedded_make_list(handle, c("Household").as_ptr())) }["ok"]["id"]
                .as_i64()
                .unwrap();

        // The units and categories the app ships with, seeded by domain's migrations.
        let units = unsafe { read(embedded_units(handle)) };
        assert!(
            !units["ok"].as_array().unwrap().is_empty(),
            "no units: {units}"
        );
        let tags = unsafe { read(embedded_tags(handle, list_id)) };
        let tag_id = tags["ok"][0]["id"].as_i64().expect("no categories");

        let item_id = unsafe {
            read(embedded_add(
                handle,
                list_id,
                c("milk").as_ptr(),
                std::ptr::null(),
            ))
        }["ok"]["id"]
            .as_i64()
            .unwrap();

        unsafe { read(embedded_attach_tag(handle, item_id, tag_id)) };
        let filed = unsafe { read(embedded_tags_on(handle, item_id)) };
        assert_eq!(
            filed["ok"].as_array().unwrap().len(),
            1,
            "filing did not stick"
        );
        unsafe { read(embedded_detach_tag(handle, item_id, tag_id)) };
        assert!(
            unsafe { read(embedded_tags_on(handle, item_id)) }["ok"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        // Reordering the walk, as one fact rather than row by row.
        let reversed: Vec<i64> = tags["ok"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .map(|t| t["id"].as_i64().unwrap())
            .collect();
        let ordered = unsafe {
            read(embedded_set_tag_order(
                handle,
                list_id,
                c(&serde_json::to_string(&reversed).unwrap()).as_ptr(),
            ))
        };
        assert!(
            ordered["error"].is_null(),
            "reordering was refused: {ordered}"
        );
        let after = unsafe { read(embedded_tags(handle, list_id)) };
        assert_eq!(
            after["ok"][0]["id"].as_i64().unwrap(),
            reversed[0],
            "the order did not take"
        );

        // A category of one's own, which is what standalone editing means.
        let made = unsafe {
            read(embedded_create_tag(
                handle,
                c("Fishmonger").as_ptr(),
                c("🐟").as_ptr(),
            ))
        };
        let mine = made["ok"]["id"].as_i64().expect("a category");
        unsafe {
            read(embedded_update_tag(
                handle,
                mine,
                c("Fish").as_ptr(),
                std::ptr::null(),
            ))
        };
        unsafe { read(embedded_delete_tag(handle, mine)) };

        // What was bought, and what to offer for a part-typed line.
        let remembered = unsafe { read(embedded_history(handle, list_id)) };
        assert!(
            !remembered["ok"].as_array().unwrap().is_empty(),
            "nothing remembered"
        );
        let offered = unsafe { read(embedded_suggestions(handle, list_id, c("mil").as_ptr())) };
        // `Milk`, capitalised, for the second time in this file: what comes back is
        // what the *server* stored, not what was typed. The clients capitalise for
        // themselves today, which is the divergence being removed.
        assert!(
            offered["ok"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == "Milk"),
            "milk was not offered for mil: {offered}"
        );

        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }

    /// A refusal is an envelope, not a null and not a crash. A client that gets null
    /// has nothing to show and nothing to log.
    #[test]
    fn a_refusal_comes_back_as_an_envelope() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };

        let answer = unsafe { read(embedded_items(handle, 9_999)) };

        assert!(
            answer["ok"].is_null(),
            "a list that does not exist answered with rows"
        );
        assert!(
            answer["error"]
                .as_str()
                .is_some_and(|said| !said.is_empty()),
            "a refusal carried no reason: {answer}"
        );

        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }

    /// Null where a handle should be is answered rather than crashed on. A client
    /// tearing down while another thread is mid-call is not a rare event.
    #[test]
    fn a_null_handle_is_answered_rather_than_crashed_on() {
        let answer = unsafe { read(embedded_lists(std::ptr::null())) };
        assert_eq!(answer["error"], "no database");

        // And the frees take null, so teardown needs no checks of its own.
        unsafe { embedded_close(std::ptr::null_mut()) };
        unsafe { embedded_free(std::ptr::null_mut()) };
        unsafe { embedded_watcher_free(std::ptr::null_mut()) };
        unsafe { embedded_stopper_free(std::ptr::null_mut()) };
    }

    /// The watching thread, as a client runs it.
    #[test]
    fn a_watcher_started_and_stopped_through_c() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };

        let watcher = unsafe { embedded_watch_lists(handle) };
        let stopper = unsafe { embedded_watcher_stopper(watcher) };

        // The client's watching thread. `usize` because a raw pointer is not `Send`,
        // and the promise being made -- one thread in `next`, any thread in `stop` --
        // is the one the documentation states.
        let parked = watcher as usize;
        let watching = std::thread::spawn(move || {
            let answer = unsafe { embedded_next_change(parked as *mut Watcher) };
            if answer.is_null() {
                return None;
            }
            Some(unsafe { read(answer) })
        });

        std::thread::sleep(std::time::Duration::from_millis(50));
        unsafe { read(embedded_make_list(handle, c("Household").as_ptr())) };

        let heard = watching.join().unwrap().expect("the watcher heard nothing");
        assert_eq!(heard["lists"], true);

        unsafe { embedded_stop(stopper) };
        unsafe { embedded_stopper_free(stopper) };
        unsafe { embedded_watcher_free(watcher) };
        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }

    /// The way across for a device that has been used, which is every device somebody
    /// actually owns. Without it the switch shows an empty app with their shopping
    /// still on disk.
    #[test]
    fn an_old_cache_is_brought_across() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };

        let everything = serde_json::json!({
            "lists": [{
                "name": "Home",
                "items": [
                    {"uuid": "milk", "name": "Milk", "amount": 2.0, "unit_id": null,
                     "done_at": null, "tag_ids": [1]},
                    {"uuid": "apples", "name": "Apples", "amount": 1.0, "unit_id": null,
                     "done_at": 1_787_908_502i64, "tag_ids": []}
                ]
            }]
        });

        let brought = unsafe { read(embedded_import(handle, c(&everything.to_string()).as_ptr())) };
        assert_eq!(brought["ok"], 2, "not everything came across: {brought}");

        let lists = unsafe { read(embedded_lists(handle)) };
        let list_id = lists["ok"][0]["id"].as_i64().unwrap();
        assert_eq!(lists["ok"][0]["name"], "Home");

        let items = unsafe { read(embedded_items(handle, list_id)) };
        let rows = items["ok"].as_array().unwrap();
        assert_eq!(rows.len(), 2);

        let milk = rows.iter().find(|r| r["name"] == "Milk").expect("milk");
        assert_eq!(milk["amount"], 2.0, "how much was lost");
        assert_eq!(
            milk["tag_ids"].as_array().map(|t| t.len()),
            Some(1),
            "what it was filed under was lost"
        );
        assert!(
            milk["done_at"].is_null(),
            "something still needed arrived crossed off"
        );

        let apples = rows.iter().find(|r| r["name"] == "Apples").expect("apples");
        assert!(
            !apples["done_at"].is_null(),
            "something crossed off arrived still needed"
        );

        // The months of history a device had built, rebuilt rather than lost -- because
        // every row came in through the service that records a use.
        let remembered = unsafe { read(embedded_history(handle, list_id)) };
        assert_eq!(
            remembered["ok"].as_array().unwrap().len(),
            2,
            "the memory did not come across: {remembered}"
        );
        let offered = unsafe { read(embedded_suggestions(handle, list_id, c("mil").as_ptr())) };
        assert!(
            offered["ok"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == "Milk"),
            "autocomplete forgot what the device knew: {offered}"
        );

        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }

    /// A tick that happened an hour ago keeps its hour. The watch is out of range in a
    /// shop and comes back with a queue; the ordering rules run on when somebody
    /// decided, not when the news arrived.
    #[test]
    fn a_tick_keeps_the_moment_it_was_made() {
        let path = scratch();
        let handle = unsafe { embedded_open(c(path.to_str().unwrap()).as_ptr()) };
        let list_id =
            unsafe { read(embedded_make_list(handle, c("Household").as_ptr())) }["ok"]["id"]
                .as_i64()
                .unwrap();
        let item_id = unsafe {
            read(embedded_add(
                handle,
                list_id,
                c("milk").as_ptr(),
                std::ptr::null(),
            ))
        }["ok"]["id"]
            .as_i64()
            .unwrap();

        let an_hour_ago = 1_787_908_502i64;
        let ticked = unsafe { read(embedded_set_done(handle, item_id, true, an_hour_ago)) };

        let stamped = ticked["ok"]["done_at"].as_str().expect("no done_at");
        assert!(
            stamped.starts_with("2026-08-28T09:15:02"),
            "the tick was stamped now rather than when it was made: {stamped}"
        );

        // And zero still means now, which is what a tap on this device means.
        unsafe { read(embedded_set_done(handle, item_id, false, 0)) };
        let again = unsafe { read(embedded_set_done(handle, item_id, true, 0)) };
        assert!(!again["ok"]["done_at"].is_null());

        unsafe { embedded_close(handle) };
        let _ = std::fs::remove_file(&path);
    }
}
