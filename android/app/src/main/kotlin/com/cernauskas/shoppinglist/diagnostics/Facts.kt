package com.cernauskas.shoppinglist.diagnostics

/**
 * What a log line is allowed to say, and the boundary that keeps shopping out of it.
 *
 * docs/self-hosting.md's S8 says the uncomfortable part out loud: a shopping list is
 * more revealing than it looks — medication, dietary restrictions, alcohol, pregnancy
 * tests, Lent, halal, kosher. None of it is anything anybody set out to collect and all
 * of it is inferable from what people type. A diagnostic log that carries item names is
 * therefore the same category of file as the database, and it is worse in one way: a
 * log is the file somebody attaches to a bug report.
 *
 * So the rule is that **`info`, `warn` and `error` can carry no personal data**, and it
 * is a rule about types rather than about care. A convention — "remember not to log
 * names" — is a rule that holds until the third person in a hurry, and then holds
 * nowhere, silently, in a file that gets emailed.
 *
 * ## How the boundary is built
 *
 * [Diagnostics.info], [Diagnostics.warn] and [Diagnostics.error] accept exactly two
 * things, and neither of them can be a free string:
 *
 *  * an [Event] — an enum, so the *name* of what happened is written in this file and
 *    cannot be assembled at a call site out of something somebody typed;
 *  * any number of [Fact]s — each a [Field] (an enum again) paired with a number, a
 *    boolean, another enum, or the *length* of a string.
 *
 * There is no `Fact.of(Field, String)` and there must never be one. That single absence
 * is the whole boundary: with no way to put a string in, there is no way for an item
 * name, a list name, an address, a token or an invite code to reach a line at those
 * levels. `AFactCannotCarryAName` in the tests holds it there by reflection, so adding
 * one fails the build rather than the review.
 *
 * `trace` and `debug` are the other half of the deal and take a lambda that may say
 * anything at all, including contents — see [Diagnostics.debug]. They are off unless
 * somebody turns them on, and turning them on says what it means in the settings screen.
 */
enum class Level(val label: String) {
    /** Everything, including what is on the lists. */
    TRACE("trace"),

    /** What the app did, including what is on the lists. */
    DEBUG("debug"),

    /** What the app did, in counts, shapes, ids, durations and outcomes. */
    INFO("info"),

    /** Something went wrong and the app carried on. */
    WARN("warn"),

    /** Something went wrong and did not. */
    ERROR("error"),
    ;

    /** Whether a line at [other] is written when this is the level in force. */
    fun admits(other: Level): Boolean = other.ordinal >= ordinal

    /**
     * Whether choosing this means agreeing to a log that holds shopping.
     *
     * Asked by the settings screen, which must warn before it is chosen rather than
     * afterwards — a warning about a file that already exists is a notification.
     */
    val revealsContents: Boolean get() = this == TRACE || this == DEBUG

    companion object {
        /**
         * What a device does before anybody has been asked.
         *
         * Not "off": a crash nobody recorded is a crash nobody can fix, and the two
         * levels above this carry no personal data by construction, so there is nothing
         * to opt into. Off is what [TRACE] and [DEBUG] are.
         */
        val default = WARN

        fun named(label: String?): Level =
            entries.firstOrNull { it.label == label } ?: default
    }
}

/**
 * What happened, from a closed list.
 *
 * An enum rather than a string for the reason the file note gives: a message assembled
 * at a call site is a message that can have a list name in it, and "log the failure with
 * its context" is exactly how that happens.
 */
enum class Event(val label: String) {
    APP_LAUNCHED("app.launched"),

    // Reading and writing, whichever backend is answering.
    BACKEND_READ("backend.read"),
    BACKEND_WRITE("backend.write"),

    /** One request to a server, with what became of it. */
    REQUEST("request"),

    /** The far end went out of reach, or came back. */
    REACHABILITY_CHANGED("reachability.changed"),

    // The queue.
    QUEUE_DRAINED("queue.drained"),
    QUEUE_REFUSED("queue.refused"),

    // The change stream, which is the thing that quietly dies in a tunnel.
    STREAM_OPENED("stream.opened"),
    STREAM_CLOSED("stream.closed"),

    // The two journeys between answering for itself and using a server.
    HANDOVER_TO_SERVER("handover.to_server"),
    HANDOVER_TO_DEVICE("handover.to_device"),

    /** A native library that should be in the APK is not — see `Embedded` and `QuickAdd`. */
    NATIVE_MISSING("native.missing"),

    /** A call across JNI answered with nothing, or with an error envelope. */
    NATIVE_FAILED("native.failed"),

    /** Room would not take a write. Swallowed by the cache, and not by this. */
    CACHE_WRITE_FAILED("cache.write_failed"),

    /** Somebody changed how much this app writes down. */
    LEVEL_CHANGED("logging.level_changed"),

    /** The log was packed up to be sent somewhere. */
    LOG_EXPORTED("log.exported"),

    METRICS_PUSHED("metrics.pushed"),
    METRICS_REFUSED("metrics.refused"),
}

/**
 * The name half of a [Fact], from a closed list for the same reason [Event] is.
 *
 * These are dimensions rather than values: `route`, `outcome`, `count`. Anything whose
 * *value* varies with what somebody typed does not belong here at all.
 */
enum class Field(val label: String) {
    ROUTE("route"),
    OUTCOME("outcome"),

    /** The HTTP status, where there was one. */
    STATUS("status"),

    /** How long it took, in milliseconds. */
    MILLIS("ms"),

    COUNT("count"),

    /** A list's id. A number the server minted; it says nothing about the shopping. */
    LIST("list"),

    /** An item's id, and the same argument. */
    ITEM("item"),

    TAG("tag"),
    SENT("sent"),
    WAITING("waiting"),
    LOST("lost"),
    REFUSED("refused"),

    /** How much is queued. */
    DEPTH("depth"),

    /** Whether the far end was reached. */
    REACHABLE("reachable"),

    /** Which backend answered — see [Mode]. */
    MODE("mode"),

    /** What kind of thing this was, where the kinds are a closed set. */
    KIND("kind"),

    LEVEL("level"),

    /** Whether the answer was a page rather than the whole thing. */
    TRUNCATED("truncated"),

    /** Which native library, by its file name. */
    LIBRARY("library"),

    /** The class of whatever was thrown. Never its message — see [Fact.failure]. */
    REASON("reason"),

    /** How many characters something was, where the characters themselves may not go. */
    LENGTH("length"),

    /** How many bytes were written or sent. */
    BYTES("bytes"),
}

/** What became of something that could have gone several ways. */
enum class Outcome {
    OK,
    UNREACHABLE,
    UNAUTHORIZED,
    NOT_ADMITTED,
    FORBIDDEN,
    NOT_FOUND,
    BAD_INPUT,
    SERVER_FAULT,

    /** The app refused before asking — no server configured, no library loaded. */
    REFUSED_HERE,
}

/** Which backend answered. */
enum class Mode { DEVICE, SERVER }

/**
 * What the shared add rules made of a typed line.
 *
 * Recorded because it is the half of a mis-add that a person cannot see: somebody types
 * a thing they believe is new, the rules recognise it as a row already on the list, and
 * what appears is a tick rather than a row. Which of the two happened is the whole
 * question, and neither of the two words says anything about the shopping.
 */
enum class Resolution { EXISTING_ROW, NEW_ROW }

/**
 * One thing worth recording, with nothing in it that belongs to anybody.
 *
 * Built only through the factories below, every one of which takes a number, a boolean
 * or another enum. The absence of a `String` factory is the redaction rule — see the
 * note on [Level].
 */
class Fact private constructor(private val field: Field, private val value: String) {

    /** `route=lists outcome=ok ms=41`, which is what a line is made of. */
    override fun toString(): String = "${field.label}=$value"

    companion object {
        fun of(field: Field, count: Int) = Fact(field, count.toString())

        fun of(field: Field, count: Long) = Fact(field, count.toString())

        fun of(field: Field, yes: Boolean) = Fact(field, yes.toString())

        /**
         * A value from a closed set somebody wrote in this repository.
         *
         * Safe because an enum constant is code rather than input: there is no way for
         * an item name to arrive as one. Every enum in this file goes through here.
         */
        fun of(field: Field, value: Enum<*>) = Fact(field, value.name.lowercase())

        /**
         * How long something was, for the strings that may not be written down.
         *
         * The one factory that touches a string, and it keeps the count and drops the
         * characters. "The line was 34 characters and the parser returned nothing" is
         * most of what a parsing bug needs, and it is not somebody's shopping.
         */
        fun length(field: Field, text: String) = Fact(field, text.length.toString())

        /**
         * What was thrown, by its class.
         *
         * **Never the message.** `ApiError.BadInput` carries whatever the server said,
         * and the server says things like the name of the row that would not update.
         * A class name is written in this codebase; a message is not.
         */
        fun failure(problem: Throwable) =
            Fact(Field.REASON, problem.javaClass.name.substringAfterLast('.'))
    }
}
