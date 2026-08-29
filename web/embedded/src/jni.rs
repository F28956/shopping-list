//! The device's server, for a caller running on the JVM.
//!
//! The same server as [`crate::ffi`], reached a different way. JNI is not a C ABI: the
//! symbol is mangled from the package, class and method together, and every string is a
//! `jstring` minted by the JVM rather than a `char *` somebody has to free. So the
//! Apple entry points cannot be reused as they are — but everything underneath them can
//! be, and is.
//!
//! **The answers are identical.** Both sides go through [`crate::ffi::enveloped`], so
//! Android reads `{"ok": …}` and `{"error": …}` exactly as the phones do, produced by
//! the same code over the same schema. That is the whole point of the embedded server:
//! not "each platform has a local database", but "every platform runs the server's own
//! code". Two envelopes would be two protocols, and the day they drifted nobody would
//! find out from a compiler.
//!
//! ## The handle
//!
//! `open` returns a `jlong` that is a `*mut Local` in disguise, and `close` gives it
//! back. Kotlin holds a number it must not invent, which is the same contract the C side
//! has and is as good as JNI gets without wrapping every call in an object.
//!
//! ## Names are load-bearing
//!
//! JNI resolves `Java_com_cernauskas_shoppinglist_data_Embedded_open` from the package,
//! class and method name together. Moving or renaming the Kotlin `Embedded` object
//! breaks the link at *run* time rather than at build time — the same warning
//! `quickadd-ffi` carries, for the same reason.

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jdouble, jlong, jstring};

use crate::ffi::{describe, enveloped, guarded};
use crate::{Change, Local, Stopper, Watcher};

// ------------------------------------------------------------------ plumbing

/// A borrowed Rust string from a `JString`, or `None` where the JVM handed over null.
fn borrow(env: &mut JNIEnv, raw: &JString) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    env.get_string(raw).ok().map(|text| text.into())
}

/// A `jstring` from anything printable, or null if the JVM will not mint one.
///
/// Null here means the JVM refused an allocation, which is not a case a caller can do
/// anything about beyond seeing `null` — the same answer the C side gives for a
/// database that would not open.
fn hand_back(env: &JNIEnv, text: &str) -> jstring {
    match env.new_string(text) {
        Ok(made) => made.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// The handle, as something safe to look at.
///
/// # Safety
///
/// `handle` must be zero, or a number [`Java_com_cernauskas_shoppinglist_data_Embedded_open`]
/// returned and nobody has closed.
unsafe fn local<'a>(handle: jlong) -> Option<&'a Local> {
    if handle == 0 {
        return None;
    }
    unsafe { (handle as *const Local).as_ref() }
}

/// Runs something against the database and hands back the envelope.
///
/// Every call below is this, which is why they are one line each: the interesting part
/// is in `domain`, and this file's only job is carrying it across a boundary.
fn answer<T: serde::Serialize>(
    env: &mut JNIEnv,
    handle: jlong,
    work: impl FnOnce(&Local) -> Result<T, crate::Error>,
) -> jstring {
    let envelope = enveloped(unsafe { local(handle) }, work);
    hand_back(env, &envelope.to_string())
}

// ------------------------------------------------------------------ the database

/// Opens the database at `path`, migrating it and finding this device's person.
///
/// Zero means it would not open — a disk that will not cooperate, or a path that is not
/// writable. Anything else must be given back to `close`.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_open(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jlong {
    let Some(path) = borrow(&mut env, &path) else {
        return 0;
    };

    // Opening runs the migrations, which is the most eventful thing this library does.
    guarded(0, || match Local::open(&std::path::PathBuf::from(path)) {
        Ok(opened) => Box::into_raw(Box::new(opened)) as jlong,
        Err(_) => 0,
    })
}

/// Closes a database opened by `open`. Zero is accepted and does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_close(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    drop(unsafe { Box::from_raw(handle as *mut Local) });
}

/// This device's person, or zero if there is no database.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_me(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let Some(local) = (unsafe { local(handle) }) else {
        return 0;
    };
    guarded(0, || local.me())
}

// ------------------------------------------------------------------ lists

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_lists(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    answer(&mut env, handle, |local| local.lists())
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_makeList(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    name: JString,
) -> jstring {
    let name = borrow(&mut env, &name).unwrap_or_default();
    answer(&mut env, handle, move |local| local.make_list(&name))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_renameList(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    id: jlong,
    name: JString,
) -> jstring {
    let name = borrow(&mut env, &name).unwrap_or_default();
    answer(&mut env, handle, move |local| local.rename_list(id, &name))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_deleteList(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.delete_list(id))
}

// ------------------------------------------------------------------ what is on one

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_items(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.items(list_id))
}

/// `uuid` is what the device called this before the server had heard of it, or null on
/// the ordinary path where the row is born here.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_add(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
    line: JString,
    uuid: JString,
) -> jstring {
    let line = borrow(&mut env, &line).unwrap_or_default();
    let uuid = borrow(&mut env, &uuid);
    answer(&mut env, handle, move |local| local.add(list_id, &line, uuid))
}

/// `at_seconds` is when the tick happened, or zero for now — a watch's tick may be an
/// hour old by the time the devices are in range, and the ordering rules run on when
/// somebody decided rather than when the news arrived.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_setDone(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: jlong,
    done: jboolean,
    at_seconds: jlong,
) -> jstring {
    let done = done != 0;
    let at = if at_seconds == 0 { None } else { Some(at_seconds) };
    answer(&mut env, handle, move |local| local.set_done(item_id, done, at))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_updateItem(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: jlong,
    name: JString,
    amount: jdouble,
    unit_id: jlong,
) -> jstring {
    let name = borrow(&mut env, &name).unwrap_or_default();
    let unit = if unit_id == 0 { None } else { Some(unit_id) };
    answer(&mut env, handle, move |local| {
        local.update(item_id, &name, amount, unit)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_deleteItem(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.delete_item(item_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_clearDone(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.clear_done(list_id))
}

// ------------------------------------------------------------------ the vocabulary

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_units(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    answer(&mut env, handle, |local| local.units())
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_tags(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.tags(list_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_tagsOn(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.tags_on(item_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_setTagOrder(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
    tag_ids_json: JString,
) -> jstring {
    // A JSON array of ids, as the C side takes it, so both platforms send the walking
    // order the same way. A list that will not parse is an empty walk rather than an
    // error: `domain` refuses that on its own terms.
    let ids: Vec<i64> = borrow(&mut env, &tag_ids_json)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    answer(&mut env, handle, move |local| {
        local.set_tag_order(list_id, &ids)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_createTag(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    name: JString,
    emoji: JString,
) -> jstring {
    let name = borrow(&mut env, &name).unwrap_or_default();
    let emoji = borrow(&mut env, &emoji);
    answer(&mut env, handle, move |local| {
        local.create_tag(&name, emoji)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_updateTag(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    id: jlong,
    name: JString,
    emoji: JString,
) -> jstring {
    let name = borrow(&mut env, &name).unwrap_or_default();
    let emoji = borrow(&mut env, &emoji);
    answer(&mut env, handle, move |local| {
        local.update_tag(id, &name, emoji)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_deleteTag(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.delete_tag(id))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_attachTag(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: jlong,
    tag_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.attach_tag(item_id, tag_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_detachTag(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: jlong,
    tag_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.detach_tag(item_id, tag_id))
}

// ------------------------------------------------------------------ what it remembers

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_history(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
) -> jstring {
    answer(&mut env, handle, move |local| local.history(list_id))
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_suggestions(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
    query: JString,
) -> jstring {
    let query = borrow(&mut env, &query).unwrap_or_default();
    answer(&mut env, handle, move |local| {
        local.suggestions(list_id, &query)
    })
}

/// Takes an old cache's contents into this database, once.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_importEverything(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    everything_json: JString,
) -> jstring {
    let Some(json) = borrow(&mut env, &everything_json) else {
        return hand_back(&env, r#"{"error":"nothing to import"}"#);
    };
    let incoming: Result<crate::Incoming, _> = serde_json::from_str(&json);
    match incoming {
        Ok(incoming) => answer(&mut env, handle, move |local| local.import(&incoming)),
        Err(problem) => hand_back(&env, &format!(r#"{{"error":"{problem}"}}"#)),
    }
}

// ------------------------------------------------------------------ somebody changed something

/// Starts watching one list. Zero means there is no database.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_watchList(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    list_id: jlong,
) -> jlong {
    let Some(local) = (unsafe { local(handle) }) else {
        return 0;
    };
    guarded(0, || Box::into_raw(Box::new(local.watch_list(list_id))) as jlong)
}

/// Starts watching which lists this person can see.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_watchLists(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let Some(local) = (unsafe { local(handle) }) else {
        return 0;
    };
    guarded(0, || Box::into_raw(Box::new(local.watch_lists())) as jlong)
}

/// **Blocks** until there is something to re-read. Null means the watch has ended.
///
/// Blocking is the point, and it is why Kotlin must call this from a background
/// dispatcher: a thread parked here holds nothing, which
/// `a_parked_watcher_does_not_hold_the_database` is the proof of.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_nextChange(
    env: JNIEnv,
    _class: JClass,
    watcher: jlong,
) -> jstring {
    if watcher == 0 {
        return std::ptr::null_mut();
    }
    let watcher = unsafe { &mut *(watcher as *mut Watcher) };

    guarded(std::ptr::null_mut(), || match watcher.wait() {
        Some(Change::List(id)) => hand_back(&env, &serde_json::json!({ "list": id }).to_string()),
        Some(Change::Lists) => hand_back(&env, &serde_json::json!({ "lists": true }).to_string()),
        None => std::ptr::null_mut(),
    })
}

/// A handle that ends a watch, usable from any thread — which is the whole reason it is
/// a separate type. `nextChange` takes the watcher exclusively, so a second thread
/// cannot reach it to stop it.
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_watcherStopper(
    _env: JNIEnv,
    _class: JClass,
    watcher: jlong,
) -> jlong {
    if watcher == 0 {
        return 0;
    }
    let watcher = unsafe { &*(watcher as *const Watcher) };
    guarded(0, || Box::into_raw(Box::new(watcher.stopper())) as jlong)
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_stop(
    _env: JNIEnv,
    _class: JClass,
    stopper: jlong,
) {
    if stopper == 0 {
        return;
    }
    unsafe { &*(stopper as *const Stopper) }.stop();
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_freeStopper(
    _env: JNIEnv,
    _class: JClass,
    stopper: jlong,
) {
    if stopper == 0 {
        return;
    }
    drop(unsafe { Box::from_raw(stopper as *mut Stopper) });
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_cernauskas_shoppinglist_data_Embedded_freeWatcher(
    _env: JNIEnv,
    _class: JClass,
    watcher: jlong,
) {
    if watcher == 0 {
        return;
    }
    drop(unsafe { Box::from_raw(watcher as *mut Watcher) });
}

/// Kept so a panic reaching here is a description rather than an abort. Unused today —
/// every JNI entry point above goes through `enveloped` or `guarded` — and referenced
/// so that removing either of those trips the compiler rather than the phone.
#[allow(dead_code)]
fn unused(panic: &Box<dyn std::any::Any + Send>) -> String {
    describe(panic)
}
