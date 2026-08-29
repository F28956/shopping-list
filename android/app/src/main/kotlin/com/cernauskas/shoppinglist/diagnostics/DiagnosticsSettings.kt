package com.cernauskas.shoppinglist.diagnostics

import android.content.Context
import android.content.SharedPreferences

/**
 * How much this device writes down, and where it sends its numbers.
 *
 * Storage and nothing else, in the shape [ServerDirectory] already uses: shared
 * preferences read through a named object, started once from `Application.onCreate`
 * before anything asks. Nothing observes storage, so a screen that changes one of these
 * re-reads it — see the settings screen.
 */
object DiagnosticsSettings {
    private const val PREFS = "diagnostics"
    private const val LEVEL = "level"
    private const val ENDPOINT = "metrics.endpoint"
    private const val HEADERS = "metrics.headers"

    private var prefs: SharedPreferences? = null

    /** Called once from `Application.onCreate`, before anything asks. */
    fun start(context: Context) {
        prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    }

    /**
     * How much goes anywhere.
     *
     * [Level.default] until somebody says otherwise, which is warnings and errors —
     * lines that carry no personal data by construction. Everything below that is off
     * on a fresh install and stays off until a person turns it on in settings.
     */
    var level: Level
        get() = Level.named(prefs?.getString(LEVEL, null))
        set(value) {
            prefs?.edit()?.putString(LEVEL, value.label)?.apply()
        }

    /**
     * Where OTLP goes, exactly as typed.
     *
     * The whole URL rather than a host, because a collector's metrics path is its own
     * business: `/v1/metrics` is the usual one and nothing says it has to be. Empty
     * means metrics are off, which is what a fresh install is.
     */
    var endpoint: String
        get() = prefs?.getString(ENDPOINT, null).orEmpty()
        set(value) {
            prefs?.edit()?.putString(ENDPOINT, value.trim())?.apply()
        }

    /**
     * Whatever the collector wants in front of a request, one `Name: value` per line.
     *
     * Free text because the answer differs per collector — an API key header here, a
     * tenant there, basic auth somewhere else. Stored rather than derived, and never
     * written to the log: this is a credential.
     */
    var headers: String
        get() = prefs?.getString(HEADERS, null).orEmpty()
        set(value) {
            prefs?.edit()?.putString(HEADERS, value)?.apply()
        }

    /** The headers as pairs, ignoring anything that is not one. */
    fun headerPairs(): List<Pair<String, String>> =
        headers.lines()
            .mapNotNull { line ->
                val at = line.indexOf(':')
                if (at <= 0) return@mapNotNull null
                val name = line.take(at).trim()
                val value = line.drop(at + 1).trim()
                if (name.isEmpty() || value.isEmpty()) null else name to value
            }

    /** Whether there is anywhere to push to at all. */
    val pushing: Boolean get() = endpoint.isNotBlank()
}
