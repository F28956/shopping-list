package com.cernauskas.shoppinglist.data

import com.cernauskas.shoppinglist.diagnostics.Diagnostics
import com.cernauskas.shoppinglist.diagnostics.Event
import com.cernauskas.shoppinglist.diagnostics.Fact
import com.cernauskas.shoppinglist.diagnostics.Field
import com.cernauskas.shoppinglist.diagnostics.Metrics
import com.cernauskas.shoppinglist.diagnostics.Mode
import com.cernauskas.shoppinglist.diagnostics.Outcome
import com.cernauskas.shoppinglist.diagnostics.Resolution
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
    /**
     * Which units may be written with no number in front of them, for the shared add
     * rules. From the bundled vocabulary rather than the cache — see
     * [Reference.bareUnitIds].
     */
    private val bareUnits: Set<Long> = emptySet(),
) : Backend {

    /** Whether the last attempt to reach the far end got there. */
    @Volatile
    private var reachedIt: Boolean = true

    override val reachable: Boolean get() = reachedIt

    /**
     * Records whether the far end answered, and says so on the way in and the way out.
     *
     * Only on the change, which is the whole value of it. Every read sets this, so
     * recording each one would be a line per request saying what the line above it said
     * — and the two moments anybody wants out of a log are the moment a phone lost the
     * server and the moment it got it back. Buried in a thousand identical lines they
     * are unfindable; as two lines with a gap between them they are the answer.
     */
    private fun reached(got: Boolean) {
        if (reachedIt == got) return
        reachedIt = got
        Metrics.reachability(got)
        Diagnostics.info(Event.REACHABILITY_CHANGED, Fact.of(Field.REACHABLE, got))
    }

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
        reached(true)
        read(Outcome.OK, answer.items.size, answer.truncated)
        // The far end answered, so anything waiting for it goes now. Here rather than in
        // a screen, because "the server is reachable" is something only this knows and
        // draining is the only sensible thing to do with that news.
        sync()
        answer
    } catch (problem: ApiError.Transport) {
        reached(false)
        // What was last seen, rather than nothing. A failed load is not evidence that
        // somebody has no lists -- which is the bug the cache exists for.
        val remembered = cache.lists()
        // The count is the interesting half. "Answered from the cache with nothing in
        // it" is the shape of the bug this whole type exists for, and it is
        // indistinguishable from a healthy empty account unless the log says which.
        read(Outcome.UNREACHABLE, remembered.size, false)
        Listing(remembered, remembered.size.toLong(), false)
    }

    override suspend fun items(list: ShoppingList): Listing<Item> = try {
        val answer = remote.items(list)
        cache.rememberItems(list, answer.items)
        reached(true)
        read(Outcome.OK, answer.items.size, answer.truncated, list)
        Diagnostics.debug(Event.BACKEND_READ, Fact.of(Field.LIST, list.id)) {
            answer.items.joinToString(", ") { "${it.name} x${it.amount}" }
        }
        sync()
        Listing(laidOver(answer.items, list), answer.total, answer.truncated)
    } catch (problem: ApiError.Transport) {
        reached(false)
        val remembered = cache.items(list)
        read(Outcome.UNREACHABLE, remembered.size, false, list)
        Listing(laidOver(remembered, list), remembered.size.toLong(), false)
    }

    /** One read, in the shape every read has: how many rows, from where, and whether it
     * was the whole answer. */
    private fun read(
        outcome: Outcome,
        count: Int,
        truncated: Boolean,
        list: ShoppingList? = null,
    ) = Diagnostics.info(
        Event.BACKEND_READ,
        Fact.of(Field.MODE, Mode.SERVER),
        Fact.of(Field.OUTCOME, outcome),
        Fact.of(Field.COUNT, count),
        Fact.of(Field.TRUNCATED, truncated),
        *listOfNotNull(list?.let { Fact.of(Field.LIST, it.id) }).toTypedArray(),
    )

    override suspend fun units(): List<Unit> = try {
        remote.units().also { cache.rememberUnits(it) }
    } catch (problem: ApiError.Transport) {
        reached(false)
        cache.seedReference(NO_LIST).first
    }

    override suspend fun tagsOrderedFor(list: ShoppingList): List<Tag> = try {
        remote.tagsOrderedFor(list).also { cache.rememberTags(list, it) }
    } catch (problem: ApiError.Transport) {
        reached(false)
        cache.seedReference(list).second
    }

    override suspend fun tagsOn(item: Item, list: ShoppingList): List<Tag> = try {
        remote.tagsOn(item, list)
    } catch (problem: ApiError.Transport) {
        reached(false)
        // What the row itself says it is filed under. Poorer than the server's answer
        // and better than none, which would read as "filed under nothing".
        val known = cache.seedReference(list).second.associateBy { it.id }
        item.tagIds.mapNotNull { known[it] }
    }

    override suspend fun suggestions(typed: String, list: ShoppingList): List<String> = try {
        remote.suggestions(typed, list)
    } catch (problem: ApiError.Transport) {
        reached(false)
        // Ranked here by the shared policy rather than offered as nothing. It used to be
        // network-only, so a phone in a shop got no suggestions at all -- and the
        // ranking was half here besides: this filtered by score and ordered by how often
        // a thing is bought, while the server ordered by how well it matched.
        QuickAdd.suggest(
            typed = typed,
            history = cache.rememberedFor(list),
            now = java.time.Instant.now().epochSecond,
        )
    }

    override suspend fun history(list: ShoppingList): List<RememberedEntry> = try {
        remote.history(list).also { cache.rememberHistory(list, it) }
    } catch (problem: ApiError.Transport) {
        reached(false)
        cache.rememberedFor(list)
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
            reached(true)
            cache.rememberLists(cache.lists() + it)
        }
    } catch (problem: ApiError.Transport) {
        reached(false)
        // A list made with no signal is a list. Where it goes in the meantime is this
        // type's business, which is what took the fallback out of the screens.
        val made = cache.makeListHere(name, ownedBy = 0)
        cache.outbox.makeList(made)
        wrote(made, Fact.of(Field.OUTCOME, Outcome.UNREACHABLE), Fact.length(Field.LENGTH, name))
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
        // What the line *means*, decided by the shared rules rather than here. They
        // answer more than the words: whether this names a row the list already has and
        // should be merged onto or put back, and what the list remembers about it.
        //
        // Queued as what it resolved to and not as typed. The server reads the line
        // again on the far side, so sending it raw would be asking two copies of the
        // rules to agree -- and the queue's own answer is what the screen has already
        // been shown.
        val decision = QuickAdd.resolve(
            line = line,
            units = runCatching { units() }.getOrDefault(emptyList()),
            bare = bareUnits,
            rows = cache.items(list),
            history = runCatching { history(list) }.getOrDefault(emptyList()),
        )

        when (decision) {
            is QuickAdd.Decision.Existing -> {
                // Already on the list. Putting it back is a tick, not a second row --
                // which is the rule a second implementation of this always loses.
                val row = cache.items(list).firstOrNull { it.uuid == decision.uuid }
                if (row != null && decision.putBack) {
                    cache.outbox.setDone(row, list, done = false)
                }
            }

            is QuickAdd.Decision.New -> {
                val uuid = UUID.randomUUID().toString()
                cache.outbox.add(uuid, nextLocalItemId(), decision.name, list)
                // Filing it takes a second operation: the wire has no field for it on an
                // add, and the queue is ordered so these land behind.
                decision.tagIds.forEach { tagId ->
                    val made = Item(id = 0, uuid = uuid, name = decision.name, amount = decision.amount)
                    cache.outbox.tag(made, list, tagId, attached = true)
                }
            }
        }

        // How long the line was and how it was read, without the line. A line the parser
        // resolved to an existing row when somebody meant a new one is the recurring
        // shape of this bug, and both halves of that are here -- see `Fact.length` for
        // why the characters are not.
        wrote(
            list,
            Fact.length(Field.LENGTH, line),
            Fact.of(
                Field.KIND,
                if (decision is QuickAdd.Decision.New) Resolution.NEW_ROW else Resolution.EXISTING_ROW,
            ),
            Fact.of(Field.COUNT, (decision as? QuickAdd.Decision.New)?.tagIds?.size ?: 0),
        )
        Diagnostics.debug(Event.BACKEND_WRITE, Fact.of(Field.LIST, list.id)) { line }

        refreshQueued()
        sync()
    }

    /**
     * One write, queued rather than sent.
     *
     * The list and the row are named by their ids, which a server minted and which say
     * nothing about the shopping. What they were *called* is at debug, where the
     * settings screen has already said what that means.
     */
    private fun wrote(list: ShoppingList, vararg facts: Fact) = Diagnostics.info(
        Event.BACKEND_WRITE,
        Fact.of(Field.MODE, Mode.SERVER),
        Fact.of(Field.LIST, list.id),
        *facts,
    )

    override suspend fun setDone(item: Item, list: ShoppingList, done: Boolean, at: Long?) {
        cache.outbox.setDone(item, list, done)
        wrote(list, Fact.of(Field.ITEM, item.id), Fact.of(Field.COUNT, if (done) 1 else 0))
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
        wrote(list, Fact.of(Field.ITEM, item.id), Fact.length(Field.LENGTH, name))
        Diagnostics.debug(Event.BACKEND_WRITE, Fact.of(Field.ITEM, item.id)) { "$name x$amount" }
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
        wrote(list, Fact.of(Field.COUNT, done.size))
        refreshQueued()
        sync()
    }

    override suspend fun delete(item: Item, list: ShoppingList) {
        cache.outbox.delete(item, list)
        wrote(list, Fact.of(Field.ITEM, item.id))
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

        // The one place the whole queue's story is in one line. A queue that will not
        // move is the complaint this app gets, and answering it means knowing which of
        // the three it was: nothing sent because there was no connection, something
        // refused and kept, or something sent and something lost behind it.
        Metrics.drained(drained.sent, drained.waiting, drained.lost.size, drained.refused)
        Metrics.queueDepth(drained.waiting)
        Diagnostics.info(
            Event.QUEUE_DRAINED,
            Fact.of(Field.SENT, drained.sent),
            Fact.of(Field.WAITING, drained.waiting),
            Fact.of(Field.LOST, drained.lost.size),
            Fact.of(Field.REFUSED, drained.refused),
        )
        // Separate and at warn, because it is the one outcome that does not heal on its
        // own -- the same reason `OfflineNote` colours it and not the other two.
        if (drained.refused) {
            Diagnostics.warn(Event.QUEUE_REFUSED, Fact.of(Field.WAITING, drained.waiting))
        }
        // The losses say what they were, in words meant for a person, so they are
        // shopping. Only where somebody has asked for a log that holds shopping.
        Diagnostics.debug(Event.QUEUE_DRAINED) { drained.lost.joinToString("; ") }

        // A drain that sent nothing while something was queued is the other way to learn
        // there is no connection, and often the first: it does not wait for a read.
        if (drained.sent > 0) {
            reached(true)
        } else if (drained.waiting > 0 && !drained.refused) {
            reached(false)
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

        var handedOver = 0
        var rows = 0

        for (list in cache.lists()) {
            if (list.id >= 0 || list.uuid in alreadyQueued) continue
            handedOver += 1
            cache.outbox.makeList(list)
            for (item in cache.items(list)) {
                rows += 1
                cache.outbox.add(item.uuid, item.id, item.name, list)
                for (tagId in item.tagIds) {
                    cache.outbox.tag(item, list, tagId, attached = true)
                }
                // After the add, and only when it is true: the wire has no field for it
                // on an add, and the queue is ordered so this lands behind.
                if (item.isDone) cache.outbox.setDone(item, list, true)
            }
        }

        // Only when something actually moved: this runs on every drain, and a line per
        // drain saying "nothing to hand over" would bury the one time it did. The
        // counts are the thing worth having, because the failure this guards against is
        // somebody adopting a server and finding half their lists.
        if (handedOver > 0) {
            Diagnostics.info(
                Event.HANDOVER_TO_SERVER,
                Fact.of(Field.COUNT, handedOver),
                Fact.of(Field.DEPTH, rows),
            )
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
