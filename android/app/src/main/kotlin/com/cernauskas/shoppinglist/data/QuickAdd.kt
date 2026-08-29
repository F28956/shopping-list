package com.cernauskas.shoppinglist.data

import android.util.Log
import com.cernauskas.shoppinglist.diagnostics.Diagnostics
import com.cernauskas.shoppinglist.diagnostics.Event
import com.cernauskas.shoppinglist.diagnostics.Fact
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json

/**
 * What `2 kg apples` means.
 *
 * One parser, shared with the server and with the Apple apps, because the alternative
 * was three of them. The same line typed into this app, the iPhone app and the web page
 * has to produce the same item -- and "has to" is not something three separate
 * implementations of a hundred lines of unit matching and number parsing were ever
 * going to manage. The forty-three cases it is tested against live in
 * `web/parsing/src/quick_add.rs`, and they are the server's tests, not a copy of them.
 *
 * **The name of this object is load-bearing.** JNI resolves the native method from the
 * package, class and method name together, so moving or renaming this breaks the link
 * at run time rather than at build time. Its other half is
 * `Java_com_cernauskas_shoppinglist_data_QuickAdd_parse` in `web/quickadd-ffi`.
 */
object QuickAdd {

    private external fun parse(line: String, unitsJson: String): String?

    /**
     * The whole of what a typed line means, not just what its words say.
     *
     * [parse] answers the smaller question -- these words are two, kilograms, apples.
     * This one answers what should *happen*: whether the line names something the list
     * already has and should be merged onto or put back, and what the list remembers
     * about it. Those are rules, and there were three copies of them until this was
     * called: the server's, the phones', and a Kotlin one that had already drifted --
     * `milk` became `Milk` on an iPhone and on the server, and stayed `milk` here.
     */
    private external fun resolve(askedJson: String): String?

    /** The remembered names worth offering, ranked by the shared policy. */
    private external fun suggest(askedJson: String): String?

    /**
     * Whether the shared parser is here.
     *
     * Not assumed. An APK missing the ABI it is running on is a packaging mistake
     * rather than something a person did, but it must not take the app down with it --
     * so the failure is recorded and every line falls back to being its own name, which
     * is what an unparseable line does anyway.
     */
    private val loaded: Boolean = try {
        System.loadLibrary("quickadd")
        true
    } catch (e: UnsatisfiedLinkError) {
        Log.e("QuickAdd", "the shared parser did not load; lines will not be read", e)
        // Written down for the same reason `Embedded` writes its own: the fallback is
        // silent and correct-looking, so `2 kg apples` quietly becomes an item called
        // "2 kg apples" and nobody reports a crash because there is not one.
        Diagnostics.error(Event.NATIVE_MISSING, Fact.failure(e))
        false
    }

    private val json = Json { ignoreUnknownKeys = true }

    /**
     * The reading of [line], with [units] as the unit names that exist.
     *
     * Never fails. A line it cannot make sense of comes back whole as the name, with an
     * amount of one -- which is what somebody typing a shopping list means by a line
     * the computer did not understand.
     */
    fun read(line: String, units: List<String>): Parsed {
        val fallback = Parsed(name = line, amount = 1.0, unit = null)
        if (!loaded) return fallback

        // The unit names go over as JSON because the boundary is JNI and an array of
        // strings would be a `jobjectArray` to build and walk by hand at both ends.
        // The serializer is named rather than inferred: `encodeToString` picks its
        // serializer from the static type, and a bare `units` here resolves to the
        // `Any` overload that throws at run time.
        val unitsJson = json.encodeToString(ListSerializer(String.serializer()), units)
        val answer = runCatching { parse(line, unitsJson) }.getOrNull() ?: return fallback
        return runCatching { json.decodeFromString<Parsed>(answer) }.getOrDefault(fallback)
    }

    /**
     * What a typed line should do to this list.
     *
     * Everything the rules need goes over at once -- the line, the units that exist,
     * the rows already here, and what the list remembers -- because which of them
     * matters depends on what the line turns out to name, and a caller cannot know that
     * in advance. See `parsing::add::resolve`.
     */
    fun resolve(
        line: String,
        units: List<Unit>,
        bare: Set<Long>,
        rows: List<Item>,
        history: List<RememberedEntry>,
    ): Decision {
        val fallback = Decision.New(name = line, amount = 1.0, unitId = null, tagIds = emptyList())
        if (!loaded) return fallback

        val asked = Asked(
            line = line,
            units = units.map { AskedUnit(it.id, it.name, bare = it.id in bare) },
            rows = rows.map { AskedRow(it.uuid, it.name, it.unitId, it.isDone) },
            history = history.map {
                AskedRemembered(it.name, it.unitId, it.amount, it.tags)
            },
        )

        val answer = runCatching { resolve(json.encodeToString(Asked.serializer(), asked)) }
            .getOrNull() ?: return fallback
        val decoded = runCatching { json.decodeFromString<Answer>(answer) }.getOrNull()
            ?: return fallback

        decoded.existing?.let { return Decision.Existing(it.uuid, it.putBack) }
        decoded.new?.let { return Decision.New(it.name, it.amount, it.unitId, it.tagIds) }
        return fallback
    }

    /**
     * Which remembered names to offer for what has been typed so far.
     *
     * The ranking is the shared policy's and not this app's. It was half here once:
     * Android filtered by score and ordered by how often a thing is bought, while the
     * server ordered by how well it matched and broke ties on use -- so `mil` offered
     * `milk` on one and `milk chocolate` on the other.
     */
    fun suggest(typed: String, history: List<RememberedEntry>, now: Long): List<String> {
        if (!loaded) return emptyList()
        val query = Query(
            query = typed,
            candidates = history.map { Candidate(it.name, it.uses, it.lastUsedAt) },
            now = now,
        )
        val answer = runCatching { suggest(json.encodeToString(Query.serializer(), query)) }
            .getOrNull() ?: return emptyList()
        return runCatching { json.decodeFromString<Offered>(answer).names }
            .getOrDefault(emptyList())
    }

    /** What the shared rules decided a line should do. */
    sealed interface Decision {
        /** It named a row the list already has: merge onto it, or put it back. */
        data class Existing(val uuid: String, val putBack: Boolean) : Decision

        /** It named something new, read the way the server would have read it. */
        data class New(
            val name: String,
            val amount: Double,
            val unitId: Long?,
            val tagIds: List<Long>,
        ) : Decision
    }

    @Serializable
    private data class Asked(
        val line: String,
        val units: List<AskedUnit>,
        val rows: List<AskedRow>,
        val history: List<AskedRemembered>,
    )

    @Serializable
    private data class AskedUnit(val id: Long, val name: String, val bare: Boolean)

    @Serializable
    private data class AskedRow(
        val uuid: String,
        val name: String,
        @SerialName("unit_id") val unitId: Long?,
        val done: Boolean,
    )

    @Serializable
    private data class AskedRemembered(
        val name: String,
        @SerialName("unit_id") val unitId: Long?,
        val amount: Double?,
        @SerialName("tag_ids") val tagIds: List<Long>,
    )

    @Serializable
    private data class Answer(val existing: AnswerExisting? = null, val new: AnswerNew? = null)

    @Serializable
    private data class AnswerExisting(
        val uuid: String,
        @SerialName("put_back") val putBack: Boolean,
    )

    @Serializable
    private data class AnswerNew(
        val name: String,
        val amount: Double,
        @SerialName("unit_id") val unitId: Long? = null,
        @SerialName("tag_ids") val tagIds: List<Long> = emptyList(),
    )

    @Serializable
    private data class Query(
        val query: String,
        val candidates: List<Candidate>,
        val now: Long,
    )

    @Serializable
    private data class Candidate(
        val name: String,
        val uses: Long,
        @SerialName("last_used_at") val lastUsedAt: Long,
    )

    @Serializable
    private data class Offered(val names: List<String>)

    @Serializable
    data class Parsed(
        val name: String,
        val amount: Double,
        /** The unit the line named, in the form it was named in, or null. */
        @SerialName("unit") val unit: String? = null,
    )
}
