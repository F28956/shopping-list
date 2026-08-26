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
data class Me(val id: Long)

@Serializable
data class Page<T>(
    val items: List<T>,
    val total: Long,
    @SerialName("has_more") val hasMore: Boolean,
)

/** Rows, and whether they are all of them. `truncated` exists to be shown: a prefix
 * presented as the whole list makes the missing rows look deleted. */
data class Listing<T>(val items: List<T>, val total: Long, val truncated: Boolean)

@Serializable
data class Invitation(val token: String)
