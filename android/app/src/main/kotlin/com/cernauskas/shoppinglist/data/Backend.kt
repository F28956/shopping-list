package com.cernauskas.shoppinglist.data

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow

/**
 * What answers this app's questions about shopping.
 *
 * The Kotlin half of `ios/Shared/Sources/Backend.swift`, and drawn for the same reason.
 * A device kept to itself and a device with a server are meant to be the same app, and
 * without this they are the same app in the worst way: standalone becomes *a server
 * that fails every request*, so every screen goes down an error path and is then told
 * the error is not real.
 *
 * ## Why the surface is split
 *
 * The two modes differ in **what they offer**, not in **how shopping works**:
 *
 *  * [Backend] is shopping. A list, what is on it, what things are called and how they
 *    are grouped. A device on its own can answer every one of these from its own
 *    database, which is what makes [LocalBackend] possible.
 *  * [Accounts] is who may sign in to a server. There is no answer to that without one.
 *  * [Sharing] is who else is on a list. A share link names a server, so with none
 *    there is no link to make.
 *
 * The second and third are **not** things a local conformer should implement badly.
 * They are things that should be *absent*, which is what the screens do by hiding them —
 * correctly, because offering to share when there is nobody to share with is a worse
 * app rather than a more uniform one. See [Capabilities].
 */
interface Backend {

    // Reading.

    suspend fun lists(): Listing<ShoppingList>
    suspend fun items(list: ShoppingList): Listing<Item>
    suspend fun units(): List<Unit>
    suspend fun tagsOrderedFor(list: ShoppingList): List<Tag>
    suspend fun tagsOn(item: Item, list: ShoppingList): List<Tag>
    suspend fun suggestions(typed: String, list: ShoppingList): List<String>
    suspend fun history(list: ShoppingList): List<RememberedEntry>

    // Lists.

    suspend fun createList(name: String): ShoppingList
    suspend fun rename(list: ShoppingList, name: String)
    suspend fun delete(list: ShoppingList)

    // What is on one.

    suspend fun add(line: String, list: ShoppingList)

    /**
     * `at` is when it *happened*, which is not always now.
     *
     * A tick made away from signal reaches the server whenever the device next has one,
     * and the ordering rules run on the moment somebody decided rather than the moment
     * the news arrived — docs/offline.md. Milliseconds since the epoch, or null for now.
     */
    suspend fun setDone(item: Item, list: ShoppingList, done: Boolean, at: Long? = null)

    suspend fun update(item: Item, list: ShoppingList, name: String, amount: Double, unitId: Long?)
    suspend fun attach(tag: Tag, item: Item, list: ShoppingList)
    suspend fun detach(tag: Tag, item: Item, list: ShoppingList)
    suspend fun clearDone(list: ShoppingList)
    suspend fun delete(item: Item, list: ShoppingList)

    // The categories, which belong to no one list.

    suspend fun setTagOrder(tags: List<Tag>, list: ShoppingList)

    // Somebody else changed something.

    /**
     * The set of lists this person can see is not what they last read.
     *
     * A nudge and never the rows: a watcher told "something moved" and re-reading cannot
     * drift, while one sent the new rows becomes a second opinion about them.
     */
    fun listChanges(): Flow<kotlin.Unit>

    /**
     * This list is not what it was, and *what* about it changed.
     *
     * The kind matters because the answers cost different amounts. A tick means re-read
     * the rows; a category renamed in settings means re-read the vocabulary, which is
     * thirty-one units and twenty-one categories. Told only that "something happened", a
     * screen has to do both — three requests per tick against a server.
     */
    fun changes(list: ShoppingList): Flow<Nudge>

    // What a screen can ask about the backend itself.

    /**
     * Whether the last attempt to reach the far end got there.
     *
     * The difference between "you have no lists" and "I could not find out", which is
     * the bug the cache was built for. A screen reads this rather than inferring it from
     * an error, because a backend that answers from its own database raises none.
     */
    val reachable: Boolean get() = true

    /** How much this backend is holding that has not reached where it is going. */
    val pending: Int get() = 0

    /** The rows on this list with work that has not been sent. */
    suspend fun unsent(list: ShoppingList): Set<String> = emptySet()

    /**
     * Sends whatever is waiting, and says what became of it.
     *
     * Only the losses are worth showing: "three changes sent" is news about plumbing,
     * while "the thing you crossed off had been deleted" is news about the list.
     */
    suspend fun sync(): SyncReport = SyncReport()
}

/** What moved. */
enum class Nudge {
    /** What is on this list: something added, ticked off, corrected, filed or removed. */
    ROWS,

    /**
     * The categories themselves — renamed, added, removed, or reordered for this list.
     * Global, and changed from a screen that belongs to no list.
     */
    CATEGORIES,
}

/** What became of a queue. */
data class SyncReport(
    val sent: Int = 0,
    val waiting: Int = 0,
    /** Something was refused and will not retry itself. The one state worth interrupting for. */
    val refused: Boolean = false,
    /** Changes that can never land — a tick against a row somebody else deleted. */
    val lost: List<String> = emptyList(),
)

/**
 * Who may sign in to this server, and who this is.
 *
 * Server-only, and deliberately not part of [Backend]: a device with no server has no
 * account to describe and nobody to admit. A screen that needs this is a screen that
 * should be absent without one.
 */
interface Accounts {
    suspend fun whoAmI(): Me
    suspend fun serverAbout(): ServerAbout
    suspend fun admissions(): List<Admitted>
    suspend fun admit(email: String, note: String?)
    suspend fun withdraw(email: String)
    suspend fun setOwner(email: String, owner: Boolean)
    suspend fun setAdmitsAnyone(open: Boolean)
}

/**
 * Who else is on a list.
 *
 * Server-only for the same reason: a share link names a server, so with none there is no
 * link to make and nobody on the other end of one.
 */
interface Sharing {
    suspend fun people(list: ShoppingList): List<Person>
    suspend fun invite(list: ShoppingList): String
    suspend fun revokeInvites(list: ShoppingList)
    suspend fun join(token: String): ShoppingList
    suspend fun remove(person: Person, list: ShoppingList)
}

/**
 * A backend that answers nothing, for a screen built before there is one.
 *
 * Not a mode and not a fallback — composition roots hold a backend as state, and this is
 * what "not built yet" looks like for the one frame before it is. Every read is empty
 * and every write is ignored, which is the honest answer to a question asked too early.
 */
object NoBackend : Backend {
    override suspend fun lists() = Listing<ShoppingList>(emptyList(), 0, false)
    override suspend fun items(list: ShoppingList) = Listing<Item>(emptyList(), 0, false)
    override suspend fun units() = emptyList<Unit>()
    override suspend fun tagsOrderedFor(list: ShoppingList) = emptyList<Tag>()
    override suspend fun tagsOn(item: Item, list: ShoppingList) = emptyList<Tag>()
    override suspend fun suggestions(typed: String, list: ShoppingList) = emptyList<String>()
    override suspend fun history(list: ShoppingList) = emptyList<RememberedEntry>()
    override suspend fun createList(name: String) = error("no backend")
    override suspend fun rename(list: ShoppingList, name: String) {}
    override suspend fun delete(list: ShoppingList) {}
    override suspend fun add(line: String, list: ShoppingList) {}
    override suspend fun setDone(item: Item, list: ShoppingList, done: Boolean, at: Long?) {}
    override suspend fun update(
        item: Item,
        list: ShoppingList,
        name: String,
        amount: Double,
        unitId: Long?,
    ) {}
    override suspend fun attach(tag: Tag, item: Item, list: ShoppingList) {}
    override suspend fun detach(tag: Tag, item: Item, list: ShoppingList) {}
    override suspend fun clearDone(list: ShoppingList) {}
    override suspend fun delete(item: Item, list: ShoppingList) {}
    override suspend fun setTagOrder(tags: List<Tag>, list: ShoppingList) {}
    override fun listChanges(): Flow<kotlin.Unit> = emptyFlow()
    override fun changes(list: ShoppingList): Flow<Nudge> = emptyFlow()
}
