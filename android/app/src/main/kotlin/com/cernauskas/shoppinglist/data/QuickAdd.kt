package com.cernauskas.shoppinglist.data

import android.util.Log
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

    @Serializable
    data class Parsed(
        val name: String,
        val amount: Double,
        /** The unit the line named, in the form it was named in, or null. */
        @SerialName("unit") val unit: String? = null,
    )
}
