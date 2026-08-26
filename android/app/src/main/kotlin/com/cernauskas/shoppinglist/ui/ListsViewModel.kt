package com.cernauskas.shoppinglist.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.ApiError
import com.cernauskas.shoppinglist.data.Cache
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
class ListsViewModel(
    private val api: Api,
    private val cache: Cache,
    private val onSignedOut: (String?) -> Unit,
) : ViewModel() {

    data class State(
        val lists: List<ShoppingList> = emptyList(),
        val total: Long = 0,
        val truncated: Boolean = false,
        val loading: Boolean = true,
        /** The server could not be reached last time we asked. Not an error and not
         * worth interrupting anybody over -- but the difference between "you have no
         * lists" and "I could not find out" has to reach the screen. */
        val offline: Boolean = false,
        /** Whether the server has ever answered this screen. What is shown while this
         * is false came out of the cache, and may be old. */
        val fresh: Boolean = false,
        val message: String? = null,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    init {
        showWhatWeHave()
        load()
        watch()
    }

    /**
     * Puts the last-loaded lists up before asking the server anything.
     *
     * The screen is never blank while a request is in flight, and on a phone with no
     * signal it is never blank at all. Guarded on [State.fresh] so a slow disk read
     * cannot land after a fast server answer and put yesterday's lists back.
     */
    private fun showWhatWeHave() = viewModelScope.launch {
        val remembered = cache.lists()
        if (remembered.isEmpty()) return@launch
        _state.update {
            if (it.fresh) it
            else it.copy(lists = remembered, total = remembered.size.toLong(), loading = false)
        }
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
            } catch (e: ApiError.NotAdmitted) {
                // Not a dropped connection. Reconnecting every three seconds to be
                // refused again is a loop nothing ends.
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

    fun load() = viewModelScope.launch {
        try {
            val listing = api.lists()
            cache.rememberLists(listing.items)
            _state.update {
                it.copy(
                    lists = listing.items,
                    total = listing.total,
                    truncated = listing.truncated,
                    loading = false,
                    offline = false,
                    fresh = true,
                )
            }
        } catch (e: ApiError.Transport) {
            // Not reported. Being out of signal is a state, not an event: a phone in a
            // basement would raise this every few seconds, and a message for each is
            // noise on top of an app that is still perfectly usable.
            val remembered = if (_state.value.lists.isEmpty()) cache.lists() else _state.value.lists
            _state.update {
                it.copy(
                    lists = remembered,
                    total = maxOf(it.total, remembered.size.toLong()),
                    loading = false,
                    offline = true,
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
        // comes back as soon as the state changes, which says it better. A refusal of
        // the account itself goes to the same screen and does say something, because
        // asking again will not change it and no list screen can explain it.
        when (e) {
            is ApiError.Unauthorized -> onSignedOut(null)
            is ApiError.NotAdmitted -> onSignedOut(e.message)
            else -> _state.update { it.copy(message = e.message) }
        }
    }
}
