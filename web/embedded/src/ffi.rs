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

    match Local::open(&PathBuf::from(path)) {
        Ok(local) => Box::into_raw(Box::new(local)),
        Err(_) => std::ptr::null_mut(),
    }
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
    match unsafe { handle.as_ref() } {
        Some(local) => local.me(),
        None => 0,
    }
}

// ------------------------------------------------------------------ lists

/// `{"ok": [ … lists … ]}`
///
/// # Safety
///
/// `handle` must be live. The answer must be given back to [`embedded_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_lists(handle: *const Local) -> *mut c_char {
    answering(handle, |local| local.lists())
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
    answering(handle, move |local| local.make_list(&name))
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
    answering(handle, move |local| local.rename_list(id, &name))
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_delete_list(handle: *const Local, id: i64) -> *mut c_char {
    answering(handle, |local| local.delete_list(id))
}

// ------------------------------------------------------------------ what is on one

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_items(handle: *const Local, list_id: i64) -> *mut c_char {
    answering(handle, |local| local.items(list_id))
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
    answering(handle, move |local| local.add(list_id, &line, uuid))
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_set_done(
    handle: *const Local,
    item_id: i64,
    done: bool,
) -> *mut c_char {
    answering(handle, |local| local.set_done(item_id, done))
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
    answering(handle, move |local| {
        local.update(item_id, &name, amount, unit)
    })
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_delete_item(handle: *const Local, item_id: i64) -> *mut c_char {
    answering(handle, |local| local.delete_item(item_id))
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_clear_done(handle: *const Local, list_id: i64) -> *mut c_char {
    answering(handle, |local| local.clear_done(list_id))
}

// ------------------------------------------------------------------ being told

/// Starts watching one list. Give the answer back to [`embedded_watcher_free`].
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_watch_list(handle: *const Local, list_id: i64) -> *mut Watcher {
    match unsafe { handle.as_ref() } {
        Some(local) => Box::into_raw(Box::new(local.watch_list(list_id))),
        None => std::ptr::null_mut(),
    }
}

/// Starts watching which lists this person can see.
///
/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn embedded_watch_lists(handle: *const Local) -> *mut Watcher {
    match unsafe { handle.as_ref() } {
        Some(local) => Box::into_raw(Box::new(local.watch_lists())),
        None => std::ptr::null_mut(),
    }
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

    match watcher.wait() {
        Some(Change::List(id)) => owned(&serde_json::json!({ "list": id })),
        Some(Change::Lists) => owned(&serde_json::json!({ "lists": true })),
        None => std::ptr::null_mut(),
    }
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
fn answering<T: serde::Serialize>(
    handle: *const Local,
    work: impl FnOnce(&Local) -> Result<T, crate::Error>,
) -> *mut c_char {
    let Some(local) = (unsafe { handle.as_ref() }) else {
        return owned(&serde_json::json!({ "error": "no database" }));
    };

    match work(local) {
        Ok(answer) => owned(&serde_json::json!({ "ok": answer })),
        Err(refusal) => owned(&serde_json::json!({ "error": refusal.to_string() })),
    }
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

        let ticked = unsafe { read(embedded_set_done(handle, item_id, true)) };
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
}
