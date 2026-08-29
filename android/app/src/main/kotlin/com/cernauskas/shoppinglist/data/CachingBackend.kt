package com.cernauskas.shoppinglist.data

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.util.UUID

/**
 * A server, with the memory and the queue a server needs.
 *
 * The Kotlin half of `ios/Store/Sources/CachingBackend.swift`. Everything about talking
 * to somebody else's machine lives here: what to show when it cannot be reached, what to
 * do with a change made in a shop, and when to try again.
 *
 * The screens above know none of it. They hold a [Backend] and cannot tell whether it is
 * this or [LocalBackend] — which is the point, and which is what took the cache and the
 * outbox out of the view models. A screen should not be the thing that knows a remote
 * can fail.
 */
class CachingBackend(
    private val remote: Api,
    private val cache: Cache,
) : Backend {

    /** Whether the last attempt to reach the far end got there. */
    @Volatile
    private var reachedIt: Boolean = true

    override val reachable: Boolean get() = reachedIt

    /**
     * What is queued, as a number a screen can show.
     *
     * Read rather than counted here, and cached: the status dot asks for it on every
     * recomposition and the honest answer is a database round trip.
     */
    @Volatile
    private var queued: Int = 0

    override val pending: Int get() = queued

    private val draining = Mutex()

    // MARK: - Reading

    override suspend fun lists(): Listing<ShoppingList> = try {
        val answer = remote.lists()
        cache.rememberLists(answer.items)
        reachedIt = true
        // The far end answered, so anything waiting for it goes now. Here rather than in
        // a screen, because "the server is reachable" is something only this knows and
        // draining is the only sensible thing to do with that news.
        sync()
        answer
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        // What was last seen, rather than nothing. A failed load is not evidence that
        // somebody has no lists -- which is the bug the cache exists for.
        val remembered = cache.lists()
        Listing(remembered, remembered.size.toLong(), false)
    }

    override suspend fun items(list: ShoppingList): Listing<Item> = try {
        val answer = remote.items(list)
        cache.rememberItems(list, answer.items)
        reachedIt = true
        sync()
        Listing(laidOver(answer.items, list), answer.total, answer.truncated)
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        val remembered = cache.items(list)
        Listing(laidOver(remembered, list), remembered.size.toLong(), false)
    }

    override suspend fun units(): List<Unit> = try {
        remote.units().also { cache.rememberUnits(it) }
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        cache.seedReference(NO_LIST).first
    }

    override suspend fun tagsOrderedFor(list: ShoppingList): List<Tag> = try {
        remote.tagsOrderedFor(list).also { cache.rememberTags(list, it) }
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        cache.seedReference(list).second
    }

    override suspend fun tagsOn(item: Item, list: ShoppingList): List<Tag> = try {
        remote.tagsOn(item, list)
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        // What the row itself says it is filed under. Poorer than the server's answer
        // and better than none, which would read as "filed under nothing".
        val known = cache.seedReference(list).second.associateBy { it.id }
        item.tagIds.mapNotNull { known[it] }
    }

    override suspend fun suggestions(typed: String, list: ShoppingList): List<String> = try {
        remote.suggestions(typed, list)
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        emptyList()
    }

    override suspend fun history(list: ShoppingList): List<RememberedEntry> = try {
        remote.history(list)
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        emptyList()
    }

    // MARK: - What is queued, laid back over what the server said
    //
    // The rule that stops a successful read visibly undoing something still queued: the
    // server has not been told, so it answers with the old state, and the row would flick
    // back for as long as the queue is stuck.

    private suspend fun laidOver(fromServer: List<Item>, list: ShoppingList): List<Item> {
        val queued = runCatching { cache.outbox.forList(list.id) }.getOrDefault(emptyList())
        if (queued.isEmpty()) return fromServer

        // Rows this device created and has not sent are not in the server's answer at
        // all, so they are carried across from what was written down. **Only** rows it
        // created: any queued operation used to qualify, which meant a tick queued
        // against a row somebody else had deleted put that row back on screen -- present
        // here, gone everywhere else, and impossible to be rid of.
        val known = fromServer.map { it.uuid }.toSet()
        val made = queued.filter { it.kind == QueuedOperation.ADD }.map { it.itemUuid }.toSet()
        val notSentYet = cache.items(list).filter { it.uuid !in known && it.uuid in made }

        var rows = fromServer + notSentYet
        val now = java.time.Instant.now().toString()

        for (operation in queued) {
            rows = when (operation.kind) {
                QueuedOperation.SET_DONE -> rows.map {
                    if (it.uuid == operation.itemUuid) {
                        it.copy(doneAt = if (operation.done) now else null)
                    } else {
                        it
                    }
                }

                QueuedOperation.DELETE -> rows.filter { it.uuid != operation.itemUuid }

                QueuedOperation.UPDATE -> rows.map {
                    if (it.uuid == operation.itemUuid) {
                        it.copy(
                            name = operation.editedName ?: it.name,
                            amount = operation.editedAmount ?: it.amount,
                        )
                    } else {
                        it
                    }
                }

                QueuedOperation.CLEAR_DONE -> rows.filter { it.uuid !in operation.sweptUuids }

                QueuedOperation.ATTACH_TAG, QueuedOperation.DETACH_TAG -> {
                    val tagId = operation.tagId
                    if (tagId == null) {
                        rows
                    } else {
                        val attaching = operation.kind == QueuedOperation.ATTACH_TAG
                        rows.map {
                            if (it.uuid != operation.itemUuid) {
                                it
                            } else {
                                val filed = it.tagIds.filter { id -> id != tagId }
                                it.copy(tagIds = if (attaching) filed + tagId else filed)
                            }
                        }
                    }
                }

                else -> rows
            }
        }
        return rows
    }

    override suspend fun unsent(list: ShoppingList): Set<String> =
        runCatching { cache.outbox.forList(list.id) }
            .getOrDefault(emptyList())
            .mapNotNull { it.itemUuid }
            .toSet()

    // MARK: - Lists

    override suspend fun createList(name: String): ShoppingList = try {
        remote.createList(name).also {
            reachedIt = true
            cache.rememberLists(cache.lists() + it)
        }
    } catch (problem: ApiError.Transport) {
        reachedIt = false
        // A list made with no signal is a list. Where it goes in the meantime is this
        // type's business, which is what took the fallback out of the screens.
        val made = cache.makeListHere(name, ownedBy = 0)
        cache.outbox.makeList(made)
        refreshQueued()
        made
    }

    override suspend fun rename(list: ShoppingList, name: String) {
        remote.rename(list, name)
    }

    override suspend fun delete(list: ShoppingList) {
        remote.delete(list)
    }

    // MARK: - What is on one

    override suspend fun add(line: String, list: ShoppingList) {
        // Queued first, then sent. That order is the whole of offline editing: a
        // decision somebody has already made should not wait on a server they cannot
        // influence.
        val uuid = UUID.randomUUID().toString()
        cache.outbox.add(uuid, localId = nextLocalItemId(), line = line, list = list)
        refreshQueued()
        sync()
    }

    override suspend fun setDone(item: Item, list: ShoppingList, done: Boolean, at: Long?) {
        cache.outbox.setDone(item, list, done)
        refreshQueued()
        sync()
    }

    override suspend fun update(
        item: Item,
        list: ShoppingList,
        name: String,
        amount: Double,
        unitId: Long?,
    ) {
        cache.outbox.update(item, list, name, amount, unitId)
        refreshQueued()
        sync()
    }

    override suspend fun attach(tag: Tag, item: Item, list: ShoppingList) {
        cache.outbox.tag(item, list, tag.id, attached = true)
        refreshQueued()
        sync()
    }

    override suspend fun detach(tag: Tag, item: Item, list: ShoppingList) {
        cache.outbox.tag(item, list, tag.id, attached = false)
        refreshQueued()
        sync()
    }

    override suspend fun clearDone(list: ShoppingList) {
        val done = cache.items(list).filter { it.isDone }
        cache.outbox.clearDone(done, list)
        refreshQueued()
        sync()
    }

    override suspend fun delete(item: Item, list: ShoppingList) {
        cache.outbox.delete(item, list)
        refreshQueued()
        sync()
    }

    override suspend fun setTagOrder(tags: List<Tag>, list: ShoppingList) {
        remote.setTagOrder(tags, list)
        cache.rememberTags(list, tags)
    }

    // MARK: - Somebody else changed something

    override fun listChanges(): Flow<kotlin.Unit> = remote.listChanges()

    /**
     * A nudge from the server, typed.
     *
     * The server's stream says only that the list moved, so everything from it is
     * [Nudge.ROWS] — the vocabulary is global and changes through a screen that belongs
     * to no list. [LocalBackend] can tell the two apart because it makes both changes
     * itself; here the first read after a nudge is what notices.
     */
    override fun changes(list: ShoppingList): Flow<Nudge> =
        remote.changes(list).map { Nudge.ROWS }

    // MARK: - The queue

    override suspend fun sync(): SyncReport = draining.withLock {
        // Anything made while there was no server at all, handed over now that there is
        // one. Cheap when there is nothing to hand over, which is the ordinary case.
        handOverIfNeeded()

        if (cache.outbox.waiting() == 0) {
            queued = 0
            return@withLock SyncReport()
        }

        val drained = cache.outbox.drain(remote)
        refreshQueued()

        // A drain that sent nothing while something was queued is the other way to learn
        // there is no connection, and often the first: it does not wait for a read.
        if (drained.sent > 0) {
            reachedIt = true
        } else if (drained.waiting > 0 && !drained.refused) {
            reachedIt = false
        }

        // Lists made here have just been given the server's own ids. Without this the
        // same list appears twice, once under each numbering.
        for (adopted in drained.adopted) {
            cache.lists().firstOrNull { it.uuid == adopted.uuid }?.let {
                cache.adopt(it, adopted.real)
            }
        }

        SyncReport(
            sent = drained.sent,
            waiting = drained.waiting,
            refused = drained.refused,
            lost = drained.lost,
        )
    }

    private suspend fun refreshQueued() {
        queued = runCatching { cache.outbox.waiting() }.getOrDefault(0)
    }

    /**
     * Lists this device made while there was no server, queued now that there is one.
     *
     * Only the lists it made: a list with a server's id is that server's. See the Apple
     * side's `handOverIfNeeded`, which this is a port of.
     */
    private suspend fun handOverIfNeeded() {
        val alreadyQueued: Set<String> = runCatching { cache.outbox.everything() }
            .getOrDefault(emptyList())
            .map { it.listUuid }
            .toSet()

        for (list in cache.lists()) {
            if (list.id >= 0 || list.uuid in alreadyQueued) continue
            cache.outbox.makeList(list)
            for (item in cache.items(list)) {
                cache.outbox.add(item.uuid, item.id, item.name, list)
                for (tagId in item.tagIds) {
                    cache.outbox.tag(item, list, tagId, attached = true)
                }
                // After the add, and only when it is true: the wire has no field for it
                // on an add, and the queue is ordered so this lands behind.
                if (item.isDone) cache.outbox.setDone(item, list, true)
            }
        }
        refreshQueued()
    }

    private suspend fun nextLocalItemId(): Long =
        // Counted down from the lowest already used rather than taken from the clock. A
        // millisecond timestamp is unique until two adds land in the same millisecond,
        // and then it is a primary key collision that rolls back the whole write -- a
        // bug this codebase has now found three times on the Apple side.
        cache.lowestItemId().coerceAtMost(0L) - 1

    private companion object {
        /**
         * The units are global, and `seedReference` wants a list to key its tags by. Any
         * list will do for the units half; this names that rather than hiding it behind
         * a magic zero at the call site.
         */
        val NO_LIST = ShoppingList(id = 0, uuid = "", name = "", ownerId = 0)
    }
}
