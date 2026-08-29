package com.cernauskas.shoppinglist.data

import android.util.Log

/**
 * The device's own server.
 *
 * Not a local database that happens to resemble the server: `web/embedded` links
 * `domain`, so this phone runs the server's own service layer over the server's own
 * schema and migrations. A list made here has been through the same rules as one made
 * on the machine in the cupboard, because it *is* the same code. That is what makes
 * standalone and server mode one app rather than two — see `web/embedded/src/lib.rs`.
 *
 * **The name of this object is load-bearing**, exactly as [QuickAdd]'s is. JNI resolves
 * the native method from package, class and method name together, so moving or renaming
 * this breaks the link at run time rather than at build time. Its other half is
 * `Java_com_cernauskas_shoppinglist_data_Embedded_*` in `web/embedded/src/jni.rs`.
 *
 * ## Every answer is an envelope
 *
 * ```json
 * {"ok": [ … ]}
 * {"error": "That list is gone."}
 * ```
 *
 * The same envelope the Apple apps read, from the same function — a caller that can
 * decode one can decode the other. Two shapes would be two protocols over one database.
 *
 * ## Handles are numbers, and must be given back
 *
 * [open] answers with a number that is a pointer in disguise, and [close] takes it back.
 * The same for watchers and their stoppers. This is as good as JNI gets without wrapping
 * every call in an object, and the rule is the C side's: free what you were given.
 *
 * ## Threads
 *
 * A handle may be used from several threads; sqlx's pool is built for it. [nextChange]
 * **blocks** until something moves, so it belongs on a background dispatcher and never
 * on the main one. A watcher may not be shared — [nextChange] takes it exclusively —
 * which is why stopping goes through a separate stopper that any thread may hold.
 */
object Embedded {

    /**
     * Whether the library is here.
     *
     * Not assumed, for the same reason [QuickAdd] does not assume it: an APK missing the
     * ABI it is running on is a packaging mistake rather than something a person did.
     * Unlike the parser there is no falling back to a poorer answer — without this there
     * is no database — so the caller's job is to notice and stay on the cached path.
     */
    val loaded: Boolean = try {
        System.loadLibrary("embedded")
        true
    } catch (e: UnsatisfiedLinkError) {
        Log.e("Embedded", "the device's own server is not in this APK", e)
        false
    }

    // The database. `open` answers 0 when it will not open.
    external fun open(path: String): Long
    external fun close(handle: Long)

    /** This device's person, or 0. */
    external fun me(handle: Long): Long

    // Lists.
    external fun lists(handle: Long): String?
    external fun makeList(handle: Long, name: String): String?
    external fun renameList(handle: Long, id: Long, name: String): String?
    external fun deleteList(handle: Long, id: Long): String?

    // What is on one.
    external fun items(handle: Long, listId: Long): String?

    /** `uuid` is what this device called it before any server heard of it, or null. */
    external fun add(handle: Long, listId: Long, line: String, uuid: String?): String?

    /** `atSeconds` is when the tick happened, or 0 for now. */
    external fun setDone(handle: Long, itemId: Long, done: Boolean, atSeconds: Long): String?

    external fun updateItem(
        handle: Long,
        itemId: Long,
        name: String,
        amount: Double,
        unitId: Long,
    ): String?

    external fun deleteItem(handle: Long, itemId: Long): String?
    external fun clearDone(handle: Long, listId: Long): String?

    // The vocabulary.
    external fun units(handle: Long): String?
    external fun tags(handle: Long, listId: Long): String?
    external fun tagsOn(handle: Long, itemId: Long): String?
    external fun setTagOrder(handle: Long, listId: Long, tagIdsJson: String): String?
    external fun createTag(handle: Long, name: String, emoji: String?): String?
    external fun updateTag(handle: Long, id: Long, name: String, emoji: String?): String?
    external fun deleteTag(handle: Long, id: Long): String?
    external fun attachTag(handle: Long, itemId: Long, tagId: Long): String?
    external fun detachTag(handle: Long, itemId: Long, tagId: Long): String?

    // What it remembers.
    external fun history(handle: Long, listId: Long): String?
    external fun suggestions(handle: Long, listId: Long, query: String): String?

    /** Takes an old cache's contents in, once. See the migration on the Apple side. */
    external fun importEverything(handle: Long, everythingJson: String): String?

    // Somebody changed something. `watchList`/`watchLists` answer 0 on failure.
    external fun watchList(handle: Long, listId: Long): Long
    external fun watchLists(handle: Long): Long

    /** **Blocks.** Null means the watch has ended. Never call this on the main thread. */
    external fun nextChange(watcher: Long): String?

    external fun watcherStopper(watcher: Long): Long
    external fun stop(stopper: Long)
    external fun freeStopper(stopper: Long)
    external fun freeWatcher(watcher: Long)
}
