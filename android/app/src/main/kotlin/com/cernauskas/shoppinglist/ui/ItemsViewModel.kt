package com.cernauskas.shoppinglist.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.ApiError
import com.cernauskas.shoppinglist.data.Item
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

/** What is on one list. */
class ItemsViewModel(
    private val api: Api,
    val list: ShoppingList,
    private val onSignedOut: () -> Unit,
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
        val message: String? = null,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    private var asking: Job? = null

    init {
        loadReference()
        load()
        watch()
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
            _state.update {
                it.copy(
                    unitList = units,
                    units = units.associate { unit -> unit.id to unit.name },
                    tags = api.tagsOrderedFor(list),
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
            _state.update { current ->
                current.copy(
                    outstanding = inShopOrder(listing.items.filter { !it.isDone }, current.tags),
                    done = listing.items.filter { it.isDone },
                    total = listing.total,
                    truncated = listing.truncated,
                    loading = false,
                )
            }
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
            } catch (_: Exception) {
                // Losing the connection is ordinary and not worth saying.
            }
            reconnecting = true
            delay(3_000)
        }
    }

    fun add(line: String) = act { api.add(line, list) }
    fun toggle(item: Item) = act { api.setDone(item, list, !item.isDone) }
    fun delete(item: Item) = act { api.delete(item, list) }
    fun clearDone() = act { api.clearDone(list) }

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
        if (e is ApiError.Unauthorized) onSignedOut() else _state.update {
            it.copy(message = e.message)
        }
    }
}
