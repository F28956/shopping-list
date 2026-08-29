package com.cernauskas.shoppinglist.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The shapes the API answers with.
 *
 * Deliberately a subset. A field added on the server does not break this app, and a
 * field removed from here is a decision rather than an accident.
 */
@Serializable
data class ShoppingList(
    val id: Long,
    /** What operations call this list, minted wherever it was made. Defaulted so a
     * server that predates the column still decodes -- and so the cache can be read
     * back by a build that has not seen one yet. */
    val uuid: String = "",
    val name: String,
    @SerialName("owner_id") val ownerId: Long,
    /** What this person may do with it. Absent means the least: read it. */
    val role: Role = Role.VIEWER,
) {
    val mayEdit: Boolean get() = role >= Role.EDITOR
}

/**
 * Ordered, so a needed role can be compared against the held one — `role >= EDITOR`
 * reads the way the service's own checks do.
 */
@Serializable
enum class Role {
    @SerialName("viewer") VIEWER,
    @SerialName("editor") EDITOR,
    @SerialName("owner") OWNER,
}

@Serializable
data class Item(
    val id: Long,
    /** What operations call this item. See [ShoppingList.uuid]. */
    val uuid: String = "",
    val name: String,
    val amount: Double,
    @SerialName("unit_id") val unitId: Long? = null,
    /** When it was crossed off, or null while it is still needed. There is no separate
     * flag, so the two cannot disagree. */
    @SerialName("done_at") val doneAt: String? = null,
    /** What it is filed under, in the order this list is walked. Empty on the routes
     * that answer with a single item: only the list route joins them. */
    @SerialName("tag_ids") val tagIds: List<Long> = emptyList(),
) {
    val isDone: Boolean get() = doneAt != null
}

@Serializable
data class Unit(val id: Long, val name: String)

@Serializable
data class Tag(
    val id: Long,
    val name: String,
    val emoji: String? = null,
    @SerialName("sort_order") val sortOrder: Long = 0,
)

@Serializable
data class Person(
    @SerialName("user_id") val userId: Long,
    val name: String? = null,
    val email: String? = null,
    val role: Role,
) {
    /** What to call them. An account can have neither a name nor an address, and
     * "Someone" at least does not pretend otherwise. */
    val shown: String get() = name ?: email ?: "Someone"
}

@Serializable
data class Me(
    val id: Long,
    /**
     * Whether this person administers *this server* — who may sign in, and who else
     * administers it.
     *
     * A fact about the server rather than about them: the same account on somebody
     * else's server is not an owner of it. Defaulted so a server older than this app,
     * where the idea did not exist, still decodes — nobody is an owner there and the
     * screen it gates simply does not appear.
     *
     * It is not a data role. An owner has no more access to anybody's lists than
     * anybody else.
     */
    @SerialName("is_owner") val isOwner: Boolean = false,
)

/** One address that may sign in to this server. */
@Serializable
data class Admitted(
    val email: String,
    /**
     * Who it turned out to be, once they signed in. `null` means nobody has used this
     * address yet — the difference between "invited" and "here".
     */
    @SerialName("user_id") val userId: Long? = null,
    val note: String? = null,
) {
    /**
     * Whether anybody has used it. The screen says so, because withdrawing an address
     * somebody is using signs them out and withdrawing one nobody has used does not.
     */
    val isInUse: Boolean get() = userId != null
}

/**
 * What a server says about itself, over the wire. The same shape as
 * [ServerDirectory.About], which is read before anybody has signed in.
 */
@Serializable
data class ServerAbout(
    val name: String,
    val version: String = "",
    /** `open`, `closed` or `unclaimed`. */
    val admission: String = "",
) {
    val admitsAnyone: Boolean get() = admission == "open"
}

@Serializable
data class Page<T>(
    val items: List<T>,
    val total: Long,
    @SerialName("has_more") val hasMore: Boolean,
)

/** Rows, and whether they are all of them. `truncated` exists to be shown: a prefix
 * presented as the whole list makes the missing rows look deleted. */
data class Listing<T>(val items: List<T>, val total: Long, val truncated: Boolean)

/**
 * One thing a list has taught the box: what it was called, and what it turned out to be.
 *
 * The memory belongs to the *list* rather than to whoever is signed in — the server
 * moved it there so a household shares one, and hands it back per list like everything
 * else. See `20260825160000_list_sharing`.
 */
@Serializable
data class RememberedEntry(
    /** The key: trimmed and lowercased, so `Milk` and `milk ` are one memory. */
    val name: String,
    /** The spelling last used, for showing back. */
    val display: String = "",
    @SerialName("unit_id") val unitId: Long? = null,
    val amount: Double? = null,
    val tags: List<Long> = emptyList(),
    val uses: Long = 0,
    /** Unix seconds, which is what the shared ranking policy wants. */
    @SerialName("last_used_at") val lastUsedAt: Long = 0,
)

@Serializable
data class Invitation(val token: String)
