package com.cernauskas.shoppinglist.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.ApiError
import com.cernauskas.shoppinglist.data.Person
import com.cernauskas.shoppinglist.data.ShoppingList
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * The lists this person can see.
 *
 * A ViewModel rather than state held in the composable: on Android a screen is torn
 * down and rebuilt for something as ordinary as a rotation, and state that lives in
 * the composition goes with it.
 */
class ListsViewModel(private val api: Api, private val onSignedOut: () -> Unit) : ViewModel() {

    data class State(
        val lists: List<ShoppingList> = emptyList(),
        val total: Long = 0,
        val truncated: Boolean = false,
        val loading: Boolean = true,
        val message: String? = null,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    init {
        load()
        watch()
    }

    /**
     * Keeps this screen in step with lists made, renamed, deleted or joined anywhere.
     *
     * A list's own stream cannot carry this: one that has just been made has no
     * watchers at all, which is why a list created on a phone never appeared on a Mac
     * left open beside it.
     */
    private fun watch() = viewModelScope.launch {
        var reconnecting = false
        while (true) {
            if (reconnecting) load()
            try {
                api.listChanges().collect { load() }
            } catch (_: Exception) {
                // Losing the connection is ordinary and not worth saying.
            }
            reconnecting = true
            delay(3_000)
        }
    }

    fun load() = viewModelScope.launch {
        try {
            val listing = api.lists()
            _state.update {
                it.copy(
                    lists = listing.items,
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

    fun create(name: String) = act { api.createList(name) }
    fun rename(list: ShoppingList, name: String) = act { api.rename(list, name) }
    fun delete(list: ShoppingList) = act { api.delete(list) }
    fun join(token: String) = act { api.join(token) }

    suspend fun people(list: ShoppingList): List<Person> = api.people(list)
    suspend fun invite(list: ShoppingList): String = api.invite(list)
    suspend fun revokeInvites(list: ShoppingList) = api.revokeInvites(list)
    suspend fun whoAmI(): Long = api.whoAmI().id
    suspend fun remove(person: Person, list: ShoppingList) = api.remove(person, list)

    fun messageShown() = _state.update { it.copy(message = null) }

    fun say(message: String) = _state.update { it.copy(message = message) }

    private fun act(work: suspend () -> Unit) = viewModelScope.launch {
        try {
            work()
            load()
        } catch (e: ApiError) {
            report(e)
        }
    }

    private fun report(e: ApiError) {
        // A signed-out session is not a message worth showing: the sign-in screen
        // comes back as soon as the state changes, which says it better.
        if (e is ApiError.Unauthorized) onSignedOut() else _state.update {
            it.copy(message = e.message)
        }
    }
}
