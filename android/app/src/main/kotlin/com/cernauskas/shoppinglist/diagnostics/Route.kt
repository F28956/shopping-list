package com.cernauskas.shoppinglist.diagnostics

/**
 * Which kind of request this was, from a closed list.
 *
 * A route *class* and never the path. Two of this app's paths carry something that
 * belongs to a person — `/api/admissions/{email}` has an address in it, and an invite
 * token would be one too if it were not deliberately sent in a body — so a metric
 * labelled with the path is a metric that leaks. Anything unrecognised becomes [OTHER]
 * rather than being passed through, which is the difference between a closed set and a
 * sanitiser somebody has to keep up to date.
 *
 * Latency is worth splitting this way and not further: `/api/sync` and `/api/lists/{id}`
 * fail for different reasons and at different speeds, while one list's items and
 * another's do not.
 */
enum class Route {
    LISTS,
    LIST,
    ITEMS,
    ITEM,
    ITEM_DONE,
    ITEM_TAGS,
    TAG_ORDER,
    HISTORY,
    UNITS,
    SYNC,
    MEMBERS,
    INVITES,
    ADMISSIONS,
    SERVER,
    ME,
    EVENTS,
    OTHER,
    ;

    companion object {
        /**
         * What a path is, structurally.
         *
         * Numbers stand in for themselves and everything else is matched literally, so
         * a segment that is neither — an address, a token — can only ever fall through
         * to [OTHER]. That is the safe direction: a new route shows up as `other` and
         * somebody adds it, rather than showing up as itself with somebody's email in it.
         */
        fun of(path: String): Route {
            val segments = path.substringBefore('?')
                .split('/')
                .filter { it.isNotEmpty() }
                .map { if (it.toLongOrNull() != null) "#" else it }

            if (segments.firstOrNull() != "api") return OTHER

            return when (segments.drop(1)) {
                listOf("lists") -> LISTS
                listOf("lists", "#") -> LIST
                listOf("lists", "#", "items") -> ITEMS
                listOf("lists", "#", "items", "#") -> ITEM
                listOf("lists", "#", "items", "done") -> ITEM_DONE
                listOf("lists", "#", "items", "#", "done") -> ITEM_DONE
                listOf("lists", "#", "items", "#", "tags") -> ITEM_TAGS
                listOf("lists", "#", "items", "#", "tags", "#") -> ITEM_TAGS
                listOf("lists", "#", "tag-order") -> TAG_ORDER
                listOf("lists", "#", "history") -> HISTORY
                listOf("lists", "#", "members") -> MEMBERS
                listOf("lists", "#", "members", "#") -> MEMBERS
                listOf("lists", "#", "members", "invites") -> INVITES
                listOf("lists", "#", "events") -> EVENTS
                listOf("units") -> UNITS
                listOf("sync") -> SYNC
                listOf("invites") -> INVITES
                listOf("server") -> SERVER
                listOf("me") -> ME
                listOf("me", "events") -> EVENTS
                else -> if (segments.getOrNull(1) == "admissions") ADMISSIONS else OTHER
            }
        }
    }
}
