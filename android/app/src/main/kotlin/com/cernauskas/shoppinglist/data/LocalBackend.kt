package com.cernauskas.shoppinglist.data

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import java.util.concurrent.Executors

/** There is no server on this device, because its database would not open. */
class NoLocalServer : Exception("This device's own list store is unavailable.")

/**
 * The device answering for itself.
 *
 * Every question this can be asked, it answers from its own database — through
 * [Embedded], which is `domain` compiled for this phone. So there is nothing to queue
 * and nothing to be offline from: a write has landed by the time it returns, and
 * `reachable` is true because the far end is this device.
 *
 * That is why [pending], [unsent] and [sync] are left at their defaults. They are not
 * unimplemented; they are answered. A queue here would be a log of everything that has
 * ever happened, written for a reader that does not exist.
 *
 * [Accounts] and [Sharing] are deliberately absent. A device with no server has no
 * account to describe and no link to make, and the screens that would offer them are
 * hidden by [Capabilities] rather than shown and then refused.
 */
class LocalBackend private constructor(private val handle: Long) : Backend, AutoCloseable {

    companion object {
        /** Where this device keeps its own database. Beside the cache, not inside it. */
        fun location(context: Context): File = File(context.filesDir, "device.sqlite")

        /**
         * Opens the device's database, or null if it cannot be opened.
         *
         * Null rather than an exception because there is nothing a caller can do about
         * it: a disk that will not hold a database is not a state this app has a screen
         * for, and the alternative is falling back to the cached path.
         */
        fun open(context: Context): LocalBackend? = openAt(location(context))

        /**
         * The same, at a path the caller names.
         *
         * For tests, which must not open the database belonging to whatever else is on
         * the device running them -- and which found out the hard way: six lists where
         * one was expected, because every case had been opening the same real file.
         */
        fun openAt(database: java.io.File): LocalBackend? {
            if (!Embedded.loaded) return null
            val handle = Embedded.open(database.path)
            return if (handle == 0L) null else LocalBackend(handle)
        }

        private val json = Json { ignoreUnknownKeys = true }

        /**
         * The device's backend, ready to use, having taken over from the old cache if it
         * had to.
         *
         * The one entry point a composition root should call. Opening is not enough on a
         * device that has been used: its lists are in the Room cache, this reads
         * `device.sqlite`, and handing the second to a screen would show an empty app
         * with somebody's shopping still on disk.
         *
         * Nothing is deleted. The old cache is left exactly as it was, which is what
         * makes this reversible: if the new path turns out to be wrong, the fallback is
         * still sitting there with everything in it.
         *
         * Null means stay on the old path -- the database would not open, or the
         * migration refused. Both are the same instruction to a caller: use what worked
         * yesterday.
         */
        suspend fun readyForUse(context: Context, cache: Cache): LocalBackend? {
            val backend = open(context) ?: return null
            if (tookOver(context)) return backend

            val waiting = cache.lists()
            if (waiting.isEmpty()) {
                // Nothing to bring, so nothing to get wrong. Marked all the same, so a
                // list made here tomorrow is not mistaken for a cache that needs
                // migrating.
                markTookOver(context, true)
                return backend
            }

            val everything = buildString {
                append("""{"lists":[""")
                waiting.forEachIndexed { at, list ->
                    if (at > 0) append(',')
                    append(json.encodeToString(Incoming.serializer(), Incoming(
                        name = list.name,
                        items = cache.items(list).map { row ->
                            IncomingItem(
                                uuid = row.uuid,
                                name = row.name,
                                amount = row.amount,
                                unitId = row.unitId,
                                doneAt = null,
                                tagIds = row.tagIds,
                            )
                        },
                    )))
                }
                append("]}")
            }

            if (!backend.takeIn(everything)) {
                backend.close()
                return null
            }
            markTookOver(context, true)
            return backend
        }

        /**
         * Whether this device has already handed its cache over.
         *
         * A flag rather than "is the new database empty", because those differ in the
         * case that matters: somebody who migrates and then deletes every list would be
         * migrated again on the next launch, and their deleted lists would come back.
         */
        private fun tookOver(context: Context): Boolean =
            context.getSharedPreferences(FLAGS, Context.MODE_PRIVATE)
                .getBoolean("device.tookOver", false)

        private fun markTookOver(context: Context, value: Boolean) {
            context.getSharedPreferences(FLAGS, Context.MODE_PRIVATE)
                .edit()
                .putBoolean("device.tookOver", value)
                .apply()
        }

        private const val FLAGS = "device"
    }

    /** One list on its way into this database. Matches `web/embedded`'s `Incoming`. */
    @kotlinx.serialization.Serializable
    private data class Incoming(val name: String, val items: List<IncomingItem>)

    @kotlinx.serialization.Serializable
    private data class IncomingItem(
        val uuid: String,
        val name: String,
        val amount: Double,
        @kotlinx.serialization.SerialName("unit_id") val unitId: Long?,
        @kotlinx.serialization.SerialName("done_at") val doneAt: Long?,
        @kotlinx.serialization.SerialName("tag_ids") val tagIds: List<Long>,
    )

    /**
     * One thread for the blocking watches.
     *
     * [Embedded.nextChange] parks until something moves, and a parked watcher holds
     * nothing — `a_parked_watcher_does_not_hold_the_database` is the proof. But it does
     * hold a *thread*, so they get their own pool rather than starving `Dispatchers.IO`
     * on a device with several lists open.
     */
    private val watching = Executors.newCachedThreadPool { runnable ->
        Thread(runnable, "embedded-watch").apply { isDaemon = true }
    }

    override fun close() {
        watching.shutdownNow()
        Embedded.close(handle)
    }

    // MARK: - The envelope

    /** The `ok` payload, or an exception carrying what the server said. */
    private fun unwrap(answer: String?): JsonElement {
        val raw = answer ?: throw ApiError.Transport(NoLocalServer())
        val envelope = json.parseToJsonElement(raw).jsonObject
        envelope["error"]?.jsonPrimitive?.content?.let { throw ApiError.BadInput(it) }
        return envelope["ok"] ?: throw ApiError.Transport(NoLocalServer())
    }

    private suspend inline fun <reified T> answering(crossinline call: () -> String?): T =
        withContext(Dispatchers.IO) { json.decodeFromJsonElement<T>(unwrap(call())) }

    /** For a call whose answer is only whether it worked. */
    private suspend fun nothing(call: () -> String?) = withContext(Dispatchers.IO) { unwrap(call()) }

    // MARK: - Reading

    override suspend fun lists(): Listing<ShoppingList> {
        val rows: List<ShoppingList> = answering { Embedded.lists(handle) }
        // No paging: a device reading its own file has no reason to withhold the second
        // hundred. The type is the server's all the same, because the screens read
        // `truncated` and should not care which backend they are talking to.
        return Listing(rows, rows.size.toLong(), false)
    }

    override suspend fun items(list: ShoppingList): Listing<Item> {
        val rows: List<Item> = answering { Embedded.items(handle, list.id) }
        return Listing(rows, rows.size.toLong(), false)
    }

    override suspend fun units(): List<Unit> = answering { Embedded.units(handle) }

    override suspend fun tagsOrderedFor(list: ShoppingList): List<Tag> =
        answering { Embedded.tags(handle, list.id) }

    override suspend fun tagsOn(item: Item, list: ShoppingList): List<Tag> =
        answering { Embedded.tagsOn(handle, item.id) }

    override suspend fun suggestions(typed: String, list: ShoppingList): List<String> =
        answering { Embedded.suggestions(handle, list.id, typed) }

    override suspend fun history(list: ShoppingList): List<RememberedEntry> =
        answering { Embedded.history(handle, list.id) }

    // MARK: - Lists

    override suspend fun createList(name: String): ShoppingList =
        answering { Embedded.makeList(handle, name) }

    override suspend fun rename(list: ShoppingList, name: String) {
        nothing { Embedded.renameList(handle, list.id, name) }
        announceLists()
    }

    override suspend fun delete(list: ShoppingList) {
        nothing { Embedded.deleteList(handle, list.id) }
        announceLists()
    }

    // MARK: - What is on one

    override suspend fun add(line: String, list: ShoppingList) {
        // No uuid: the row is born here and is named here. The parameter exists for the
        // migration, where a row already has a name every queued operation uses.
        nothing { Embedded.add(handle, list.id, line, null) }
    }

    override suspend fun setDone(item: Item, list: ShoppingList, done: Boolean, at: Long?) {
        val seconds = at?.let { it / 1_000 } ?: 0L
        nothing { Embedded.setDone(handle, item.id, done, seconds) }
    }

    override suspend fun update(
        item: Item,
        list: ShoppingList,
        name: String,
        amount: Double,
        unitId: Long?,
    ) {
        nothing { Embedded.updateItem(handle, item.id, name, amount, unitId ?: 0L) }
    }

    override suspend fun attach(tag: Tag, item: Item, list: ShoppingList) {
        nothing { Embedded.attachTag(handle, item.id, tag.id) }
    }

    override suspend fun detach(tag: Tag, item: Item, list: ShoppingList) {
        nothing { Embedded.detachTag(handle, item.id, tag.id) }
    }

    override suspend fun clearDone(list: ShoppingList) {
        nothing { Embedded.clearDone(handle, list.id) }
    }

    override suspend fun delete(item: Item, list: ShoppingList) {
        nothing { Embedded.deleteItem(handle, item.id) }
    }

    // MARK: - The categories

    override suspend fun setTagOrder(tags: List<Tag>, list: ShoppingList) {
        val ids = tags.joinToString(",", "[", "]") { it.id.toString() }
        nothing { Embedded.setTagOrder(handle, list.id, ids) }
        announceCategories()
    }

    // MARK: - Somebody changed something

    override fun listChanges(): Flow<kotlin.Unit> = watch { Embedded.watchLists(handle) }

    /**
     * This list, and the categories it is walked by.
     *
     * Two sources, because `domain` announces only one of them. `service::tags::attach`
     * and `detach` announce on the list's channel — those are rows. Creating, renaming,
     * removing or reordering a category announces **nothing**, because a category
     * belongs to no list and there is no channel for "the vocabulary moved". So this
     * says it itself: every tag mutation here tells whoever is watching.
     */
    override fun changes(list: ShoppingList): Flow<Nudge> = callbackFlow {
        val fromRows = parked({ Embedded.watchList(handle, list.id) }) { trySend(Nudge.ROWS) }

        // Registered before anything suspends, so a category edited immediately after
        // the watch begins is not lost -- which is precisely the case: a caller starts
        // watching and then changes something.
        val token = java.util.UUID.randomUUID().toString()
        categoryWatchers[token] = { trySend(Nudge.CATEGORIES); kotlin.Unit }

        awaitClose {
            categoryWatchers.remove(token)
            fromRows.cancel(true)
        }
    }.flowOn(Dispatchers.IO)

    /**
     * Whoever is watching wants to know the vocabulary moved.
     *
     * Keyed so a screen that goes away stops being told; an unbounded, never-pruned list
     * of callbacks is a leak that only shows after somebody has opened forty lists.
     */
    private val categoryWatchers =
        java.util.concurrent.ConcurrentHashMap<String, () -> kotlin.Unit>()

    private val listWatchers =
        java.util.concurrent.ConcurrentHashMap<String, () -> kotlin.Unit>()

    private fun announceCategories() = categoryWatchers.values.forEach { it() }

    private fun announceLists() = listWatchers.values.forEach { it() }

    /** `domain`'s own broadcast channel, as a flow. */
    private fun watch(start: () -> Long): Flow<kotlin.Unit> = callbackFlow {
        val work = parked(start) { trySend(kotlin.Unit) }
        val token = java.util.UUID.randomUUID().toString()
        listWatchers[token] = { trySend(kotlin.Unit); kotlin.Unit }
        awaitClose {
            listWatchers.remove(token)
            work.cancel(true)
        }
    }.flowOn(Dispatchers.IO)

    /**
     * A thread parked in `nextChange`, calling back whenever it wakes.
     *
     * The blocking call is the whole point -- it is how `domain` says something moved
     * without this polling -- and it is why these get their own pool rather than a
     * dispatcher other work is queued on.
     */
    private fun parked(start: () -> Long, moved: () -> kotlin.Any?) =
        watching.submit {
            val watcher = start()
            if (watcher == 0L) return@submit
            try {
                while (true) {
                    Embedded.nextChange(watcher) ?: break
                    moved()
                }
            } finally {
                Embedded.freeWatcher(watcher)
            }
        }

    // MARK: - Handing this device to a server

    /**
     * Everything on this device, for a caller about to tell a server about it.
     *
     * The mirror of [takeIn]. Nothing is deleted: `device.sqlite` is left exactly as it
     * was, so giving the server up again brings it all back — which is what makes
     * adopting one safe before anybody has proved the server works.
     */
    suspend fun everythingHere(): List<Pair<ShoppingList, List<Item>>>? = try {
        lists().items.map { list -> list to items(list).items }
    } catch (_: Exception) {
        // One list that will not read is not a reason to hand over the others and
        // quietly lose this one.
        null
    }

    /**
     * Puts what this device holds where a server can be told about it.
     *
     * The mirror of [readyForUse]. That carries the old cache *into* `device.sqlite`
     * when somebody stops using a server; this carries it back out when somebody adopts
     * one. Without it, choosing a server shows an empty account with everything still on
     * disk -- the queue is built by walking the cache, and on a device that has only
     * ever answered for itself that cache is empty.
     *
     * Nothing is deleted, for the same reason as the other direction: `device.sqlite` is
     * left exactly as it was, so giving the server up brings it all back. That is what
     * makes this safe to run before anybody has proved the server works.
     */
    suspend fun handOverToAServer(cache: Cache): Boolean {
        val taken = everythingHere() ?: return false
        // The copy left behind by the takeover goes first: it is the same shopping under
        // different uuids, and queueing both tells the server about it twice.
        cache.forgetLocalLists()
        cache.takeIn(taken)
        return true
    }

    /** Takes an old cache's contents in, through `domain`'s own services. */
    suspend fun takeIn(everythingJson: String): Boolean = withContext(Dispatchers.IO) {
        try {
            unwrap(Embedded.importEverything(handle, everythingJson))
            true
        } catch (_: Exception) {
            false
        }
    }
}
