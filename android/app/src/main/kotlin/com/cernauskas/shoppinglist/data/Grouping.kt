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
 * Every tag an item carries, in the order this list is walked.
 *
 * The first is the one that decided where the row sits; the rest are true of it too,
 * and a row that shows only the first looks exactly like a row filed under one thing.
 *
 * [tags] must arrive in the list's order, as the service resolves it — the same rule as
 * [primaryTag]. Tags the item carries that this list does not know about are dropped
 * rather than shown out of place.
 */
fun tagsOn(item: Item, tags: List<Tag>): List<Tag> {
    val placed = tags.withIndex().associate { (at, tag) -> tag.id to at }
    return item.tagIds.mapNotNull { id -> placed[id] }.sorted().map { tags[it] }
}

/** A tag in one glyph: its emoji, or its name when it has none. */
val Tag.mark: String get() = emoji?.takeIf { it.isNotBlank() } ?: name

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
 * A unit is never hidden. `unit` — the one that means counted rather than measured —
 * used to print as nothing, on the grounds that it says nothing a number does not. It
 * turned out to say one thing that matters: that the row has a unit at all. A row
 * showing nothing was indistinguishable from a row that had lost one, and the only way
 * to tell was to look in the database.
 *
 * Nothing at all is left for the rows that genuinely have no unit: those predate the
 * rule that gives every item one, and one of something unmeasured is still a row where
 * "1" would be noise dressed as information. The same rule every other client follows,
 * so they do not disagree about what a row says.
 */
fun Item.measure(units: Map<Long, String>): String? {
    val unit = unitId?.let { units[it] }

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
        // The fragment, after the `#`. A browser never sends a fragment to a server,
        // so a token there is written into no access log and no proxy log on the way
        // to somebody's home server, for the week it stays valid.
        // An *empty* fragment is not a token, and it does not mean there is none: a
        // link ending in a bare `#` still carries one in its path. Returning null there
        // is why `…/join/TOKEN#` worked on an iPhone and not here.
        val fragment = trimmed.substringAfterLast('#', "")
        if (fragment.isNotEmpty()) return fragment
        // Whatever is left once the empty fragment comes off, or `…/join#` reads as a
        // path ending in `join#` and is not recognised as the join page at all.
        val withoutFragment = trimmed.substringBefore('#')
        // The older shape, with the token in the path. Still read, so that a link sent
        // before a server was updated keeps working in somebody's inbox.
        //
        // The scheme comes off first. Without that, "http://localhost:8080/" reduces
        // to "localhost:8080" and a bare origin is read as an invitation -- so an app
        // pointed at a server, with no link at all, would try to redeem its own host.
        val afterHost = withoutFragment.substringAfter("://").substringAfter('/', "")
        return afterHost.trimEnd('/').substringAfterLast('/')
            .takeIf { it.isNotEmpty() && it != "join" }
    }
    return if (trimmed.contains(' ') || trimmed.contains('/')) null else trimmed
}

/**
 * The server a share link came from, if it named one.
 *
 * C7. A share link is the ordinary way a second person arrives — often on a phone with
 * no app on it yet — and it carries its own origin. Offering that address turns the
 * worst first run in the product, "somebody sent me a list and the app is asking me for
 * a URL", into one tap.
 *
 * **Offered and never adopted.** A link is a bearer credential from an untrusted sender,
 * and pointing an app at a host because a message said so is not something to do without
 * showing the host.
 *
 * `null` for a bare token, which names nothing.
 */
fun serverAddressIn(pasted: String): ServerAddress? {
    val trimmed = pasted.trim()
    if (!trimmed.contains("://")) return null

    val uri = android.net.Uri.parse(trimmed)
    val scheme = uri.scheme ?: return null
    val host = uri.host ?: return null

    // Rebuilt from the parts rather than trimmed from the string, so that whatever
    // `ServerAddress` refuses is refused here too — one set of rules about what an
    // address is, and it lives there.
    val origin = buildString {
        append(scheme).append("://").append(host)
        if (uri.port != -1) append(':').append(uri.port)
    }

    return ServerAddress.parse(origin).getOrNull()
}
