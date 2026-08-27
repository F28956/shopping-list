package com.cernauskas.shoppinglist.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.ApiError
import com.cernauskas.shoppinglist.data.Cache
import com.cernauskas.shoppinglist.data.Identity
import com.cernauskas.shoppinglist.data.Item
import com.cernauskas.shoppinglist.data.QueuedOperation
import com.cernauskas.shoppinglist.data.QuickAdd
import com.cernauskas.shoppinglist.data.tagId
import com.cernauskas.shoppinglist.data.ServerDirectory
import com.cernauskas.shoppinglist.data.done
import com.cernauskas.shoppinglist.data.editedAmount
import com.cernauskas.shoppinglist.data.editedName
import com.cernauskas.shoppinglist.data.sweptUuids
import com.cernauskas.shoppinglist.data.ItemDraft
import com.cernauskas.shoppinglist.data.ShoppingList
import com.cernauskas.shoppinglist.data.Tag
import com.cernauskas.shoppinglist.data.Unit as ShoppingUnit
import com.cernauskas.shoppinglist.data.inShopOrder
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Instant

/** What is on one list. */
class ItemsViewModel(
    private val api: Api,
    private val cache: Cache,
    val list: ShoppingList,
    private val onSignedOut: (Identity.Departure) -> Unit,
) : ViewModel() {

    data class State(
        val outstanding: List<Item> = emptyList(),
        val done: List<Item> = emptyList(),
        val units: Map<Long, String> = emptyMap(),
        /** In the order that decides where items sit — resolved per person, per list. */
        val tags: List<Tag> = emptyList(),
        val unitList: List<ShoppingUnit> = emptyList(),
        val suggestions: List<String> = emptyList(),
        val total: Long = 0,
        val truncated: Boolean = false,
        val loading: Boolean = true,
        /** The server could not be reached last time we asked -- see
         * [ListsViewModel.State.offline]. */
        val offline: Boolean = false,
        /** Whether the server has ever answered this screen. */
        val fresh: Boolean = false,
        /** How many changes made here are still waiting to be sent. */
        val waiting: Int = 0,
        /** The rows carrying one of them. Marked quietly on the row itself rather than
         * with a banner: it is a detail about that line, not news about the app. */
        val unsent: Set<String> = emptySet(),
        /** Something was refused and will not be retried on its own. The one state of
         * the three in docs/offline.md that is worth interrupting somebody for. */
        val refused: Boolean = false,
        val message: String? = null,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    private var asking: Job? = null

    /** Guards against a drain and a reload calling each other round in a circle. */
    private var draining = false

    init {
        showWhatWeHave()
        loadReference()
        load()
        watch()
        keepTrying()
        viewModelScope.launch { refreshUnsent() }
    }

    /**
     * Tries the queue again, every so often, for as long as anything is in it.
     *
     * A load drains on success, and a load happens when the change stream reconnects --
     * which is the right moment when there is a stream to reconnect. It is the wrong
     * thing to depend on entirely: a queue is work somebody is waiting for, and hanging
     * it on somebody else editing the list means a tick made in a shop can sit there
     * until it happens.
     *
     * Ten seconds, and only while there is something to send: an empty queue costs one
     * comparison and no request at all.
     */
    private fun keepTrying() = viewModelScope.launch {
        while (true) {
            delay(10_000)
            drain()
        }
    }

    /**
     * Puts the list up as it was last seen, before asking anything.
     *
     * Reference data first and in the same breath: an item read out of the cache with
     * no units and no tags is a bare name in no category, which is a worse answer than
     * the one the shop actually needs.
     */
    private fun showWhatWeHave() = viewModelScope.launch {
        val units = cache.units()
        val tags = cache.tags(list)
        val remembered = cache.items(list)
        _state.update {
            if (it.fresh) it
            else it.copy(
                unitList = units.ifEmpty { it.unitList },
                units = if (units.isEmpty()) it.units else units.associate { u -> u.id to u.name },
                tags = tags.ifEmpty { it.tags },
                outstanding = if (remembered.isEmpty()) it.outstanding
                    else inShopOrder(remembered.filter { item -> !item.isDone }, tags),
                done = if (remembered.isEmpty()) it.done else remembered.filter { item -> item.isDone },
                total = maxOf(it.total, remembered.size.toLong()),
                loading = it.loading && remembered.isEmpty(),
            )
        }
    }

    /**
     * Units and tags, once.
     *
     * They are seeded by migration and change when the server is deployed, not when
     * somebody crosses something off — and [load] runs on every change anyone makes to
     * this list, from any device.
     */
    private fun loadReference() = viewModelScope.launch {
        try {
            val units = api.units()
            val tags = api.tagsOrderedFor(list)
            cache.rememberUnits(units)
            cache.rememberTags(list, tags)
            _state.update {
                it.copy(
                    unitList = units,
                    units = units.associate { unit -> unit.id to unit.name },
                    tags = tags,
                )
            }
        } catch (_: ApiError) {
            // Not reported: without these, rows lose their measure and their order,
            // which is a poorer list rather than no list.
            //
            // And on a device with no server there is no answer coming at all, so the
            // bundled copy stands in. Without it `3 kg potatoes` parsed to an item
            // called "kg potatoes" -- the amount came off, the unit did not, because
            // there was no list of units for the parser to recognise `kg` against.
            val (units, tags) = cache.seedReference(list)
            _state.update {
                it.copy(
                    unitList = it.unitList.ifEmpty { units },
                    units = it.units.ifEmpty { units.associate { unit -> unit.id to unit.name } },
                    tags = it.tags.ifEmpty { tags },
                )
            }
        }
    }

    fun load() = viewModelScope.launch {
        try {
            val listing = api.items(list)
            cache.rememberItems(list, listing.items)
            val shown = withUnsent(listing.items)
            _state.update { current ->
                current.copy(
                    outstanding = inShopOrder(shown.filter { !it.isDone }, current.tags),
                    done = shown.filter { it.isDone },
                    total = listing.total,
                    truncated = listing.truncated,
                    loading = false,
                    offline = false,
                    fresh = true,
                )
            }
            // The server is reachable, so anything waiting can go now. This is what
            // makes the queue drain on its own: coming back into signal reconnects the
            // change stream, the stream triggers a load, and the load sends what has
            // been waiting. Nobody has to reopen the screen.
            drain()
        } catch (e: ApiError.Transport) {
            // See ListsViewModel.load: no signal is a state, not an event. What is on
            // screen stays there -- it is the last thing the server said, and saying
            // nothing instead would be the emptiness this whole change is about.
            _state.update { it.copy(loading = false, offline = true) }
        } catch (e: ApiError) {
            report(e)
            _state.update { it.copy(loading = false) }
        }
    }

    /**
     * Keeps this screen in step with the phone, the watch, the Mac and the browser.
     *
     * Reconnects for as long as the screen is up: a stream that has ended is
     * indistinguishable from a list where nothing is happening, and silently showing
     * a stale list is what this exists to prevent.
     */
    private fun watch() = viewModelScope.launch {
        var reconnecting = false
        while (true) {
            if (reconnecting) load()
            try {
                api.changes(list).collect { load() }
            } catch (e: ApiError.NotAdmitted) {
                onSignedOut(Identity.Departure.Refused(e.message))
                return@launch
            } catch (_: ApiError.Forbidden) {
                return@launch
            } catch (_: Exception) {
                // Losing the connection is ordinary and not worth saying.
            }
            reconnecting = true
            delay(3_000)
        }
    }

    // Every change below follows the same shape: change the screen, queue the
    // operation, try to send. That order is the whole of offline editing -- a decision
    // somebody has already made should not wait on a server they cannot influence.
    //
    // The queue is what makes the promise good. If the send fails the operation stays
    // in it, and the next successful load from anywhere in the app sends it. See
    // [com.cernauskas.shoppinglist.data.Outbox].

    /**
     * Puts something on the list.
     *
     * The row appears at once, under a uuid minted here and a **negative id**. The
     * negative id never leaves the device: it is a placeholder so the screen has
     * something to key on, and the uuid is what the operation actually names. When the
     * add lands, the reload replaces it with the server's row -- same uuid, real id.
     *
     * The line is read here, and it used to be shown as typed until the server sent
     * back what it had made of it. That was fine when there was always a server; on a
     * device with none, `2 kg apples` stayed an item called "2 kg apples" for ever --
     * no amount, no unit, and nothing coming to fix it.
     *
     * It is not a second parser. [QuickAdd] is the server's own, compiled for the
     * phone, so what appears in this row is what the server would have said and the
     * reload has nothing to correct.
     */
    fun add(line: String) = viewModelScope.launch {
        val uuid = java.util.UUID.randomUUID().toString()
        // `units` is id-to-name, which is the shape the rest of this screen wants.
        val units = _state.value.units
        val parsed = QuickAdd.read(line.trim(), units.values.toList())
        val local = Item(
            id = -(Instant.now().toEpochMilli()),
            uuid = uuid,
            name = parsed.name,
            amount = parsed.amount,
            // Matched case-insensitively, because the parser answers with the unit as
            // the line spelled it -- somebody who typed `2 KG apples` named the same
            // unit as somebody who typed `kg`.
            unitId = units.entries
                .firstOrNull { (_, name) -> name.equals(parsed.unit, ignoreCase = true) }
                ?.key,
        )
        cache.outbox.add(uuid, local.id, line, list)
        show { rows -> rows + local }
        drain()
    }

    fun toggle(item: Item) = viewModelScope.launch {
        val done = !item.isDone
        cache.outbox.setDone(item, list, done)
        show { rows ->
            rows.map { if (it.uuid == item.uuid) it.copy(doneAt = if (done) Instant.now().toString() else null) else it }
        }
        drain()
    }

    fun delete(item: Item) = viewModelScope.launch {
        cache.outbox.delete(item, list)
        show { rows -> rows.filter { it.uuid != item.uuid } }
        drain()
    }

    /**
     * Empties the trolley of what is on this screen, and says so on the wire.
     *
     * The rows are named rather than described. "Everything that is done" replayed an
     * hour later would also take what somebody else ticked off meanwhile, which nobody
     * asked for -- docs/offline.md (4).
     */
    fun clearDone() = viewModelScope.launch {
        val done = _state.value.done
        if (done.isEmpty()) return@launch
        cache.outbox.clearDone(done, list)
        show { rows -> rows.filter { row -> done.none { it.uuid == row.uuid } } }
        drain()
    }

    fun save(item: Item, edit: ItemDraft.Edit, attached: List<Tag>) = viewModelScope.launch {
        cache.outbox.update(item, list, edit.name, edit.amount, edit.unitId)

        // Queued, not sent. Filing used to go straight to the network and apologise if
        // it could not get there, which offline lost the change and on a device with no
        // server meant the aisle picker never worked at all.
        val before = attached.map { it.id }.toSet()
        _state.value.tags.filter { it.id in edit.tagIds && it.id !in before }
            .forEach { cache.outbox.tag(item, list, it.id, attached = true) }
        attached.filter { it.id !in edit.tagIds }
            .forEach { cache.outbox.tag(item, list, it.id, attached = false) }

        show { rows ->
            rows.map {
                if (it.uuid == item.uuid) {
                    it.copy(
                        name = edit.name,
                        amount = edit.amount,
                        unitId = edit.unitId,
                        // What the editor was closed on. It used to keep the row's old
                        // filing and wait for the server to send the new one back,
                        // which on a device with no server was a wait that never ended.
                        tagIds = edit.tagIds.toList(),
                    )
                } else {
                    it
                }
            }
        }

        drain()
    }

    /**
     * Rewrites what is on screen, and remembers it.
     *
     * One place, so an optimistic change cannot end up on the screen but not in the
     * cache -- which is how a change survives the app being killed before it is sent.
     */
    private suspend fun show(change: (List<Item>) -> List<Item>) {
        val rows = change(_state.value.outstanding + _state.value.done)
        cache.rememberItems(list, rows)
        _state.update { current ->
            current.copy(
                outstanding = inShopOrder(rows.filter { !it.isDone }, current.tags),
                done = rows.filter { it.isDone },
                total = rows.size.toLong(),
            )
        }
        refreshUnsent()
    }

    /**
     * Sends what is queued, then says what became of it.
     *
     * Only losses are said out loud. "Three changes sent" is news about plumbing; "the
     * thing you crossed off had been deleted" is news about the list, and it is the one
     * case where somebody watched themselves do something that did not happen.
     */
    private suspend fun drain() {
        if (draining) return

        // Read the queue back even when there is nothing to send, and *before* the
        // early return. The lists screen drains the same queue on its own -- it has to,
        // because the app opens there -- so this screen's count can go stale the moment
        // that happens. Returning early without refreshing left "3 changes waiting to
        // be sent" on a screen whose queue had been empty for minutes.
        refreshUnsent()
        if (cache.outbox.waiting() == 0) return

        draining = true
        val drained = cache.outbox.drain(api)
        draining = false

        refreshUnsent()
        // A drain that sent nothing while something was queued is the other way to
        // learn there is no connection, and often the first: it does not wait for a
        // reload to fail.
        _state.update {
            it.copy(
                refused = drained.refused,
                offline = if (drained.sent > 0) false else it.offline || drained.waiting > 0 && !drained.refused,
            )
        }
        drained.lost.firstOrNull()?.let { lost ->
            _state.update { it.copy(message = lost) }
        }
        // Read back what the server made of it -- which is also how a row created here
        // gets its real id. Re-entry stops at the guard above: the queue is empty now.
        if (drained.sent > 0) load().join()
    }

    /**
     * Which rows are still waiting on a server, and how many.
     *
     * Both are empty on a device with no server, and not because nothing is queued --
     * on a list that is only ever this device's, *everything* is queued and nothing
     * ever leaves. Marking every row as waiting says the app is behind on work it means
     * to do, and it isn't: the list is already exactly what it should be. The queue is
     * still kept, because a server added later is owed every one of them.
     */
    private suspend fun refreshUnsent() {
        if (ServerDirectory.isOnDeviceOnly) {
            _state.update { it.copy(unsent = emptySet(), waiting = 0) }
            return
        }
        val queued = cache.outbox.forList(list.id)
        _state.update {
            it.copy(unsent = queued.map { op -> op.itemUuid }.toSet(), waiting = queued.size)
        }
    }

    /**
     * The server's answer with this device's unsent changes laid back over it.
     *
     * Without this a successful load would visibly undo work that is still queued: the
     * server has not been told, so it answers with the old state, and the rows would
     * flick back for as long as the queue is stuck.
     *
     * Rows this device created and has not sent are not in the server's answer at all,
     * so they are carried across from what is already on screen rather than rebuilt.
     */
    private suspend fun withUnsent(fromServer: List<Item>): List<Item> {
        val queued = cache.outbox.forList(list.id)
        if (queued.isEmpty()) return fromServer

        // Only rows this device *created* and has not sent are carried across. Any
        // queued operation used to qualify, which meant a tick queued against a row
        // somebody else had deleted put that row back on screen as a ghost -- present
        // here, gone everywhere else, and impossible to get rid of.
        val known = fromServer.map { it.uuid }.toSet()
        val onScreen = _state.value.outstanding + _state.value.done
        val made = queued.filter { it.kind == QueuedOperation.ADD }.map { it.itemUuid }.toSet()
        val notSentYet = onScreen.filter { it.uuid !in known && it.uuid in made }

        var rows = fromServer + notSentYet
        val now = Instant.now().toString()

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

                QueuedOperation.CLEAR_DONE ->
                    rows.filter { it.uuid !in operation.sweptUuids }

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

    fun setTagOrder(tags: List<Tag>) = viewModelScope.launch {
        try {
            api.setTagOrder(tags, list)
            _state.update { it.copy(tags = api.tagsOrderedFor(list)) }
            load()
        } catch (e: ApiError) {
            report(e)
        }
    }

    /**
     * What this item is filed under, for the editor.
     *
     * Asked of the server, because the list route sends tag ids and the editor wants
     * the tags themselves. With no connection the ids are enough: this screen already
     * holds every tag on the list, so the answer is a lookup rather than a request.
     *
     * It used to be the bare call, and opening the editor with no signal threw a
     * transport error out of a coroutine nobody was catching -- which is to say, it
     * crashed the app.
     */
    suspend fun tagsOn(item: Item): List<Tag> = try {
        api.tagsOn(item, list)
    } catch (_: ApiError) {
        _state.value.tags.filter { it.id in item.tagIds }
    }

    /**
     * Asks again for what has just been typed.
     *
     * Cancelled on every keystroke, so a slow answer for `mil` cannot arrive after a
     * fast one for `milk` and put the wrong list back. The matching and the cap are
     * the service's; this decides only when to ask.
     */
    fun suggest(typed: String) {
        asking?.cancel()
        val wanted = typed.trim()
        if (wanted.isEmpty()) {
            _state.update { it.copy(suggestions = emptyList()) }
            return
        }
        asking = viewModelScope.launch {
            delay(150)
            val found = try {
                api.suggestions(wanted, list)
            } catch (_: ApiError) {
                emptyList()
            }
            _state.update { it.copy(suggestions = found) }
        }
    }

    fun clearSuggestions() = _state.update { it.copy(suggestions = emptyList()) }

    fun messageShown() = _state.update { it.copy(message = null) }

    private fun act(work: suspend () -> Unit) = viewModelScope.launch {
        try {
            work()
            load()
        } catch (e: ApiError) {
            report(e)
        }
    }

    private fun report(e: ApiError) {
        // See ListsViewModel.report.
        when (e) {
            is ApiError.Unauthorized -> onSignedOut(Identity.Departure.Refused())
            is ApiError.NotAdmitted -> onSignedOut(Identity.Departure.Refused(e.message))
            else -> _state.update { it.copy(message = e.message) }
        }
    }
}
