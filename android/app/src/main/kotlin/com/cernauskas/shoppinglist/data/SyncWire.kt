package com.cernauskas.shoppinglist.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The shapes `POST /api/sync` speaks.
 *
 * Everything names rows by **uuid**, never by id. That is the whole reason the column
 * exists: an item added with no signal has no id, and will not have one until this
 * route answers -- but it has been called by this uuid since the moment somebody typed
 * it, and every operation queued behind it says the same.
 */
@Serializable
data class Batch(val operations: List<SyncOperation>)

@Serializable
data class SyncOperation(
    /** What this operation is called. The server records it, so a resend is a no-op. */
    val id: String,
    /** When this device says it happened, RFC 3339. The server clamps it forward only:
     * behind is believed, ahead is not. */
    val at: String,
    /** The list, by uuid. */
    val list: String,
    val kind: String,
    val item: String? = null,
    val items: List<String>? = null,
    val line: String? = null,
    val name: String? = null,
    val amount: Double? = null,
    @SerialName("unit_id") val unitId: Long? = null,
    /** The row as this device saw it, for an edit made against a copy. What decides
     * between renaming a row and splitting one. */
    val seen: SeenOn? = null,
    val done: Boolean? = null,
    /** The aisle an `attach_tag` or `detach_tag` names. */
    @SerialName("tag_id") val tagId: Long? = null,
)

@Serializable
data class SeenOn(
    val name: String,
    val amount: Double,
    @SerialName("unit_id") val unitId: Long? = null,
)

@Serializable
data class Replayed(val operations: List<AppliedOperation>)

/**
 * What became of one operation.
 *
 * `outcome` is `applied`, `already_applied` or `refused`. A refusal carries `why`, and
 * every one of them is a sentence an app can put in front of somebody.
 */
@Serializable
data class AppliedOperation(
    val id: String,
    val outcome: String,
    val item: Item? = null,
    /**
     * The list a `make_list` produced. Absent on every other operation — a device that
     * made a list offline knows what it called it and not what the server does, and
     * this is where it finds out.
     */
    val list: ShoppingList? = null,
    val why: String? = null,
) {
    val landed: Boolean get() = outcome == APPLIED || outcome == ALREADY_APPLIED

    /**
     * Whether the device should keep this operation rather than forget it.
     *
     * Only for work refused because the person is no longer allowed on the list. That
     * is the one refusal that may un-refuse itself: if they are invited back it is
     * still here to send, and nothing was quietly binned behind them -- see
     * docs/offline.md (8). Everything else will be refused forever.
     */
    val keepForLater: Boolean get() = outcome == REFUSED && why == NOT_ALLOWED

    /** What to tell somebody who watched themselves do this. */
    val lost: String? get() = when {
        outcome != REFUSED -> null
        why == GONE -> "Someone had already deleted it."
        why == LIST_GONE -> "That list has been deleted."
        why == NOT_ALLOWED -> "You are no longer on that list."
        else -> "The server would not accept it."
    }

    companion object {
        const val APPLIED = "applied"
        const val ALREADY_APPLIED = "already_applied"
        const val REFUSED = "refused"
        const val GONE = "gone"
        const val LIST_GONE = "list_gone"
        const val NOT_ALLOWED = "not_allowed"
    }
}
