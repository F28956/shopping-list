package com.cernauskas.shoppinglist.data

/**
 * The tag an item is filed under: the first of its tags in this list's order.
 *
 * The one that decides where the item sits, which is why a row showing a single tag
 * should show this one — any other names a place the item is not.
 *
 * [tags] must arrive in the list's order, as the service resolves it. Position in that
 * list is the whole rule; `sortOrder` is one global opinion that puts every shop-name
 * tag last and can never let `urgent` lead.
 */
fun primaryTag(item: Item, tags: List<Tag>): Tag? {
    val placed = tags.withIndex().associate { (at, tag) -> tag.id to at }
    return item.tagIds.mapNotNull { id -> placed[id]?.let { it to tags[it] } }
        .minByOrNull { it.first }
        ?.second
}

/**
 * Outstanding items in the order this list is walked.
 *
 * Grouped by the tag that leads, then flattened: the order is what matters, and the
 * groups themselves are not shown — each row carries its own tag instead. An untagged
 * item falls last.
 */
fun inShopOrder(items: List<Item>, tags: List<Tag>): List<Item> {
    val placed = tags.withIndex().associate { (at, tag) -> tag.id to at }

    // Stable: within a position the server's own order stands, and that order is its
    // answer about what is outstanding and what is done.
    return items.sortedBy { item ->
        primaryTag(item, tags)?.let { placed[it.id] } ?: Int.MAX_VALUE
    }
}

/** "2" rather than "2.0", "1.5" left alone. Counts are whole far more often than not,
 * and a trailing ".0" reads as a measurement rather than a count. */
fun Double.asAmount(): String =
    if (this == Math.floor(this) && !isInfinite() && Math.abs(this) < 1e15) {
        toLong().toString()
    } else {
        toString()
    }

/**
 * How much of it, or nothing at all.
 *
 * One of something unmeasured is the default and the commonest case, so printing "1"
 * on most rows is noise dressed as information — the same rule every other client
 * follows, so they do not disagree about what a row says.
 */
fun Item.measure(units: Map<Long, String>): String? {
    // `unit` is the unit that means "counted, not measured", and it is what an item
    // added without one is given. It says nothing a number does not, so it prints as
    // nothing: six eggs, not "6 unit".
    val unit = unitId?.let { units[it] }?.takeIf { it != "unit" }

    return when {
        amount == 1.0 && unit == null -> null
        unit == null -> amount.asAmount()
        else -> "${amount.asAmount()} $unit"
    }
}

/**
 * What the item editor has been typed into, and whether it can be saved.
 *
 * Separate from the screen because this is the part with rules in it, and it decides
 * whether the app sends the server something it will refuse.
 */
data class ItemDraft(
    val name: String,
    val amount: String,
    val unitId: Long?,
    /** Held here rather than applied as they are tapped, so Cancel undoes tags along
     * with everything else. */
    val tagIds: Set<Long>,
) {
    /** The values to send, or null when what is typed is not a saveable item. Null is
     * also what greys out Save, so there is one rule rather than two that can drift. */
    val validated: Edit?
        get() {
            val trimmed = name.trim()
            // A comma is the decimal separator across most of Europe and the keyboard
            // offers whichever the phone is set to.
            val quantity = amount.trim().replace(',', '.').toDoubleOrNull()
            if (trimmed.isEmpty() || quantity == null || quantity <= 0 || !quantity.isFinite()) {
                return null
            }
            return Edit(trimmed, quantity, unitId, tagIds)
        }

    data class Edit(
        val name: String,
        val amount: Double,
        val unitId: Long?,
        val tagIds: Set<Long>,
    )

    companion object {
        fun of(item: Item, attached: List<Tag>) = ItemDraft(
            name = item.name,
            amount = item.amount.asAmount(),
            unitId = item.unitId,
            tagIds = attached.map { it.id }.toSet(),
        )
    }
}

/**
 * The token inside a share link.
 *
 * Whoever receives one pastes the whole link, or just the token, or the link with a
 * stray space around it. All three mean the same request, and asking somebody to trim
 * it themselves is asking them to do the computer's job.
 */
fun tokenIn(pasted: String): String? {
    val trimmed = pasted.trim()
    if (trimmed.isEmpty()) return null

    if (trimmed.contains("://")) {
        return trimmed.trimEnd('/').substringAfterLast('/').ifEmpty { null }
    }
    return if (trimmed.contains(' ') || trimmed.contains('/')) null else trimmed
}
