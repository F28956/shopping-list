package com.cernauskas.shoppinglist.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.ApiError
import com.cernauskas.shoppinglist.data.Cache
import com.cernauskas.shoppinglist.data.Item
import com.cernauskas.shoppinglist.data.done
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
    private val onSignedOut: (String?) -> Unit,
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
        val unsent: Set<Long> = emptySet(),
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
        // `load` drains on success, so what was queued in the shop yesterday goes as
        // soon as the first request gets through.
        viewModelScope.launch { refreshUnsent() }
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
                onSignedOut(e.message)
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

    fun add(line: String) = act { api.add(line, list) }

    /**
     * Crosses something off, or puts it back, whether or not there is a connection.
     *
     * The screen changes first and the server is told second. That order is the whole
     * of offline editing: a tick in a shop with no signal is a decision the person has
     * already made, and an app that waits for a server before showing it has made them
     * wait for something they cannot influence.
     *
     * The queue is what makes the promise good. If the send fails the operation stays
     * in it, and the next drain -- on the next load, or the next time this screen opens
     * -- sends it. See [com.cernauskas.shoppinglist.data.Outbox].
     */
    fun toggle(item: Item) = viewModelScope.launch {
        val done = !item.isDone
        cache.outbox.setDone(item, list, done)
        applyLocally(item, done)
        cache.rememberItems(list, _state.value.outstanding + _state.value.done)
        drain()
    }

    fun delete(item: Item) = act { api.delete(item, list) }
    fun clearDone() = act { api.clearDone(list) }

    /**
     * Sends what is queued, then says what became of it.
     *
     * Only the losses are said out loud. "Three changes sent" is news about plumbing;
     * "the thing you crossed off had been deleted" is news about the list, and it is
     * the one case where somebody watched themselves do something that did not happen.
     */
    private suspend fun drain() {
        if (draining || cache.outbox.waiting() == 0) return
        draining = true
        val drained = cache.outbox.drain(api)
        draining = false

        refreshUnsent()
        if (drained.dropped.isNotEmpty()) {
            _state.update {
                it.copy(message = "Someone had already deleted what you were ${drained.dropped.first()}.")
            }
        }
        // Read back what the server made of it. Re-entry stops here: the queue this
        // guards on is empty now.
        if (drained.sent > 0) load().join()
    }

    /** Moves a row between the two sections, without asking anybody. */
    private fun applyLocally(item: Item, done: Boolean) {
        val changed = item.copy(doneAt = if (done) Instant.now().toString() else null)
        _state.update { current ->
            val rest = (current.outstanding + current.done).filter { it.id != item.id }
            current.copy(
                outstanding = inShopOrder(rest.filter { !it.isDone } + listOfNotNull(changed.takeIf { !done }), current.tags),
                done = rest.filter { it.isDone } + listOfNotNull(changed.takeIf { done }),
                unsent = current.unsent + item.id,
            )
        }
    }

    private suspend fun refreshUnsent() {
        val queued = cache.outbox.forList(list.id)
        _state.update { it.copy(unsent = queued.map { op -> op.itemId }.toSet(), waiting = queued.size) }
    }

    /**
     * The server's answer with this device's unsent changes laid back over it.
     *
     * Without this a successful load would visibly undo a tick that is still in the
     * queue -- the server has not been told yet, so it answers with the old state, and
     * the row would flick back for as long as the queue is stuck.
     */
    private suspend fun withUnsent(items: List<Item>): List<Item> {
        val queued = cache.outbox.forList(list.id)
        if (queued.isEmpty()) return items
        val now = Instant.now().toString()
        var result = items
        for (operation in queued) {
            result = result.map {
                if (it.id == operation.itemId) {
                    it.copy(doneAt = if (operation.done) now else null)
                } else {
                    it
                }
            }
        }
        return result
    }

    fun save(item: Item, edit: ItemDraft.Edit, attached: List<Tag>) = act {
        api.update(item, list, edit.name, edit.amount, edit.unitId)

        // Tags have their own routes, so this is the diff. Only what changed is sent:
        // re-attaching a tag an item already has is a conflict, and detaching one it
        // never had is a miss.
        val before = attached.map { it.id }.toSet()
        _state.value.tags.filter { it.id in edit.tagIds && it.id !in before }
            .forEach { api.attach(it, item, list) }
        attached.filter { it.id !in edit.tagIds }
            .forEach { api.detach(it, item, list) }
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

    suspend fun tagsOn(item: Item): List<Tag> = api.tagsOn(item, list)

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
            is ApiError.Unauthorized -> onSignedOut(null)
            is ApiError.NotAdmitted -> onSignedOut(e.message)
            else -> _state.update { it.copy(message = e.message) }
        }
    }
}
