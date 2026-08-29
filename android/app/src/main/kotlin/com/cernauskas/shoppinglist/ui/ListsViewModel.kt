package com.cernauskas.shoppinglist.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.cernauskas.shoppinglist.data.Accounts
import com.cernauskas.shoppinglist.data.ApiError
import com.cernauskas.shoppinglist.data.Backend
import com.cernauskas.shoppinglist.data.Identity
import com.cernauskas.shoppinglist.data.Person
import com.cernauskas.shoppinglist.data.Sharing
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
    private val backend: Backend,
    /**
     * Who else is on a list, when there is a server. Null on a device kept to itself,
     * where there is no link to make and nobody on the other end of one -- and where
     * [Capabilities] hides the controls rather than offering them to be refused.
     */
    private val sharing: Sharing?,
    /** Who this is, when there is a server. Null for the same reason as [sharing]. */
    private val accounts: Accounts?,
    private val onSignedOut: (Identity.Departure) -> Unit,
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
        /** How many changes are waiting, anywhere. The app opens here, so this is where
         * somebody first sees whether the device is in step. */
        val waiting: Int = 0,
        val message: String? = null,
    )

    private val _state = MutableStateFlow(State())
    val state: StateFlow<State> = _state.asStateFlow()

    init {
        load()
        watch()
        countWhatIsWaiting()
    }

    /**
     * Keeps the dot honest while somebody is looking at it.
     *
     * Cheap, and necessary: every items screen drains the same queue, so this screen's
     * idea of what is waiting goes stale the moment one of them succeeds.
     */
    private fun countWhatIsWaiting() = viewModelScope.launch {
        while (true) {
            _state.update { it.copy(waiting = backend.pending) }
            delay(2_000)
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
                backend.listChanges().collect { load() }
            } catch (e: ApiError.NotAdmitted) {
                // Not a dropped connection. Reconnecting every three seconds to be
                // refused again is a loop nothing ends.
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

    fun load() = viewModelScope.launch {
        try {
            val listing = backend.lists()
            _state.update {
                it.copy(
                    lists = listing.items,
                    total = listing.total,
                    truncated = listing.truncated,
                    loading = false,
                    // Not an error and not worth interrupting anybody over -- but the
                    // difference between "you have no lists" and "I could not find out"
                    // has to reach the screen. The backend is the only thing that knows,
                    // because one that answers from its own database raises nothing.
                    offline = !backend.reachable,
                    // What was shown came from somewhere real either way; `fresh` says
                    // whether the far end confirmed it.
                    fresh = backend.reachable,
                    waiting = backend.pending,
                )
            }
        } catch (e: ApiError) {
            report(e)
            _state.update { it.copy(loading = false) }
        }
    }

    /**
     * Makes a list, wherever it can.
     *
     * The server first, because a list made online should arrive with an id and no
     * queue behind it. A transport failure is not an error here and never shows one:
     * no signal and no server are the same state, and writing the list down locally is
     * what the person asked for either way. It is queued, and the queue is what carries
     * it to a server if one ever appears.
     *
     * This is S1 — the app is useful before it has anywhere to send anything.
     */
    fun create(name: String) = viewModelScope.launch {
        try {
            // Wherever it can. With a server this goes there; with none it is written
            // down here and is a list all the same. That choice is the backend's, and it
            // is why this method no longer has two halves.
            backend.createList(name)
            load()
        } catch (e: ApiError) {
            report(e)
        }
    }

    fun rename(list: ShoppingList, name: String) = act { backend.rename(list, name) }

    fun delete(list: ShoppingList) = act { backend.delete(list) }

    // Sharing, and only where there is somebody to share with.
    //
    // Reaching any of these without a server is a bug rather than a state: the controls
    // are hidden by `Capabilities`, so getting here means something is out of step. It
    // used to be `sharing?.join(token)` -- a silent no-op, which is exactly how it
    // presented when a view model built in standalone outlived the choice of a server.
    // Somebody pasted a code and nothing happened at all.
    fun join(token: String) = act {
        val sharing = sharing ?: return@act say("This device is not using a server.")
        sharing.join(token)
    }

    suspend fun people(list: ShoppingList): List<Person> = sharing?.people(list) ?: emptyList()

    suspend fun invite(list: ShoppingList): String =
        sharing?.invite(list) ?: throw ApiError.BadInput("This device is not using a server.")

    suspend fun revokeInvites(list: ShoppingList) { sharing?.revokeInvites(list) }

    suspend fun remove(person: Person, list: ShoppingList) { sharing?.remove(person, list) }

    suspend fun whoAmI(): Long = accounts?.whoAmI()?.id ?: 0L

    /** One list's change stream, for a screen that wants to follow a single list. */
    fun watchList(list: ShoppingList) = backend.changes(list)

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
            is ApiError.Unauthorized -> onSignedOut(Identity.Departure.Refused())
            is ApiError.NotAdmitted -> onSignedOut(Identity.Departure.Refused(e.message))
            else -> _state.update { it.copy(message = e.message) }
        }
    }
}
