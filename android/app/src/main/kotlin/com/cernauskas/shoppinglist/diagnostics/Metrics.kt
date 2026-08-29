package com.cernauskas.shoppinglist.diagnostics

import com.cernauskas.shoppinglist.BuildConfig
import com.cernauskas.shoppinglist.data.Capabilities
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * Numbers about this app, pushed to a collector somebody else runs.
 *
 * ## Only where there is a server
 *
 * A device answering for itself collects nothing and sends nothing, and the guard is
 * [Capabilities.syncing] rather than a preference. That is the honest reading of what
 * standalone *is*: there is no far end, so there is no latency, no queue, no stream and
 * nothing to be offline from — every measurement below is a measurement of a
 * relationship that does not exist. It is also the promise the app makes on the settings
 * screen, and a promise enforced at one point is a promise rather than a habit.
 *
 * Somebody who has a server has a machine of their own and an operator who is themselves
 * — see docs/self-hosting.md, S8 — so pushing to a collector they configured is telling
 * themselves about their own phone. It is off until they type an address.
 *
 * ## Why this is not an SDK
 *
 * OTLP/HTTP is a POST of a JSON document, and what is below is that document. The
 * OpenTelemetry Android SDK brings a metric pipeline, a span pipeline, a context
 * propagator and several megabytes of it, for an app that wants eight instruments and
 * has an HTTP client already. Delta temporality means nothing has to be reconciled
 * across a restart either, which is most of what an SDK is carrying.
 *
 * ## What may be in here
 *
 * The same rule as the log, for the same reason and by the same means: every metric name
 * is written in this file, and every attribute value is an enum constant. Nothing that
 * came from a person can reach a label — see [Route], which is where a path stops being
 * a path.
 */
object Metrics {

    // MARK: - What is being counted

    /** A metric and the attributes it was recorded under. */
    private data class Series(val metric: String, val attributes: List<Pair<String, String>>)

    /** Counts since the last push. Delta, so a push that fails costs one window rather
     * than corrupting a running total. */
    private val counts = LinkedHashMap<Series, Long>()

    /** The latest reading of something that is a level rather than a total. */
    private val levels = LinkedHashMap<Series, Long>()

    /** Durations, bucketed. */
    private val durations = LinkedHashMap<Series, Buckets>()

    private val lock = Any()

    /** Milliseconds, and chosen for what actually happens: a home server on the same
     * wifi answers in tens, one over the internet in hundreds, and a phone in a tunnel
     * gives up in tens of thousands. */
    private val BOUNDS = doubleArrayOf(5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0)

    private class Buckets {
        val counts = LongArray(BOUNDS.size + 1)
        var total = 0.0
        var seen = 0L

        fun record(millis: Long) {
            seen += 1
            total += millis
            var at = BOUNDS.indexOfFirst { millis <= it }
            if (at < 0) at = BOUNDS.size
            counts[at] += 1
        }
    }

    // MARK: - Recording

    /**
     * The one gate, and every recording goes through it.
     *
     * Here rather than at the call sites, for the reason `Outbox.queue` gives about its
     * own guard: a caller that has to remember is a caller that will not, and this one
     * is a promise rather than an optimisation.
     */
    private inline fun whenSyncing(work: () -> kotlin.Unit) {
        if (!Capabilities.current.syncing) return
        work()
    }

    private fun count(metric: String, attributes: List<Pair<String, String>> = emptyList(), by: Long = 1) =
        whenSyncing {
            synchronized(lock) {
                val series = Series(metric, attributes)
                counts[series] = (counts[series] ?: 0L) + by
            }
        }

    private fun level(metric: String, value: Long) = whenSyncing {
        synchronized(lock) { levels[Series(metric, emptyList())] = value }
    }

    /** The app was opened. */
    fun launched() = count("shopping.app.launches")

    /** One request, how long it took and what became of it. */
    fun request(route: Route, outcome: Outcome, millis: Long) = whenSyncing {
        val series = Series(
            "shopping.request.duration",
            listOf("route" to route.name.lowercase(), "outcome" to outcome.name.lowercase()),
        )
        synchronized(lock) { durations.getOrPut(series) { Buckets() }.record(millis) }
    }

    /** How much is queued, as a level rather than a total. */
    fun queueDepth(waiting: Int) = level("shopping.queue.depth", waiting.toLong())

    /**
     * What became of a drain.
     *
     * Counted by what happened to the operations rather than by the drain, because a
     * drain that sent forty and lost one is not the same event as one that lost forty.
     */
    fun drained(sent: Int, waiting: Int, lost: Int, refused: Boolean) = whenSyncing {
        if (sent > 0) count("shopping.queue.operations", listOf("outcome" to "sent"), sent.toLong())
        if (lost > 0) count("shopping.queue.operations", listOf("outcome" to "lost"), lost.toLong())
        if (refused) count("shopping.sync.refusals")
        count(
            "shopping.queue.drains",
            listOf("outcome" to if (waiting > 0 && sent == 0) "stuck" else "drained"),
        )
    }

    /** The change stream came up or went down — the health that matters most, because a
     * stream that dies silently is what stops a screen ever learning it is back. */
    fun stream(opened: Boolean) =
        count("shopping.stream.transitions", listOf("state" to if (opened) "opened" else "closed"))

    /** The far end went out of reach, or came back. */
    fun reachability(reachable: Boolean) =
        count(
            "shopping.offline.transitions",
            listOf("state" to if (reachable) "online" else "offline"),
        )

    // MARK: - Pushing

    private val client = OkHttpClient.Builder()
        // Short. A collector that is not answering must not hold a coroutine open long
        // enough for the next window to pile up behind it.
        .callTimeout(15, TimeUnit.SECONDS)
        .build()

    /**
     * Pushes every minute, for as long as the scope lives.
     *
     * Started from `Application`, which is the only place with a lifetime that matches:
     * a scope tied to a screen would stop counting whenever somebody backgrounded the
     * app, which is exactly when a queue is interesting.
     */
    fun pushEvery(scope: CoroutineScope, seconds: Long = 60) {
        scope.launch {
            while (isActive) {
                delay(seconds * 1_000)
                push()
            }
        }
    }

    /**
     * Sends what has accumulated, and forgets it whether or not it arrived.
     *
     * Delta temporality is what makes that acceptable: a failed push costs one window
     * and the next one is complete, where a retry queue would be a second outbox — an
     * unbounded one, holding numbers, on a phone.
     */
    suspend fun push() {
        if (!Capabilities.current.syncing) return
        val endpoint = DiagnosticsSettings.endpoint
        if (endpoint.isBlank()) return

        val document = drain() ?: return

        withContext(Dispatchers.IO) {
            val request = Request.Builder()
                .url(endpoint)
                .post(document.toString().toRequestBody(JSON))
                .apply {
                    DiagnosticsSettings.headerPairs().forEach { (name, value) ->
                        header(name, value)
                    }
                }
                .build()

            try {
                client.newCall(request).execute().use { response ->
                    if (response.isSuccessful) {
                        Diagnostics.info(Event.METRICS_PUSHED)
                    } else {
                        // The status and nothing else. A collector's error body says
                        // what it did not like about the document, and this app does
                        // not know what is in one of those.
                        Diagnostics.warn(
                            Event.METRICS_REFUSED,
                            Fact.of(Field.STATUS, response.code),
                        )
                    }
                }
            } catch (problem: IOException) {
                Diagnostics.warn(Event.METRICS_REFUSED, Fact.failure(problem))
            }
        }
    }

    /** Takes everything counted since the last time, leaving the counters empty. */
    private fun drain(): JsonObject? {
        val takenCounts: Map<Series, Long>
        val takenLevels: Map<Series, Long>
        val takenDurations: Map<Series, Buckets>

        synchronized(lock) {
            if (counts.isEmpty() && levels.isEmpty() && durations.isEmpty()) return null
            takenCounts = LinkedHashMap(counts).also { counts.clear() }
            // Levels are not deltas and are not cleared: a queue that is empty and stays
            // empty is a reading worth having every minute, not a gap in a graph.
            takenLevels = LinkedHashMap(levels)
            takenDurations = LinkedHashMap(durations).also { durations.clear() }
        }

        val now = System.currentTimeMillis() * 1_000_000
        val began = (windowBegan.takeIf { it != 0L } ?: now).also { windowBegan = now }

        val metrics = buildJsonArray {
            takenCounts.forEach { (series, value) ->
                add(sum(series, value, began, now))
            }
            takenLevels.forEach { (series, value) ->
                add(gauge(series, value, now))
            }
            takenDurations.forEach { (series, buckets) ->
                add(histogram(series, buckets, began, now))
            }
        }

        return document(metrics)
    }

    /** When the window being reported started, in nanoseconds. */
    @Volatile
    private var windowBegan: Long = 0

    // MARK: - The document
    //
    // OTLP/HTTP with a JSON body, which is a shape rather than a protocol: the fields
    // below are `opentelemetry.proto.metrics.v1` as JSON, and a collector that accepts
    // protobuf on the same port accepts this on `/v1/metrics` with the JSON content
    // type. Numbers that are 64-bit go as strings, which is the protobuf JSON mapping
    // and not a quirk of this.

    private fun document(metrics: JsonArray): JsonObject = buildJsonObject {
        putJsonArray("resourceMetrics") {
            addJsonObject {
                putJsonObject("resource") {
                    putJsonArray("attributes") {
                        add(attribute("service.name", "shopping-list-android"))
                        add(attribute("service.version", BuildConfig.VERSION_NAME))
                        // Deliberately nothing that identifies the install. An id here
                        // would separate one phone's series from another's, and it would
                        // also be the one label in this document that is about a person
                        // rather than about the software. Whoever runs the collector can
                        // tell their phones apart by the fact that they own them.
                    }
                }
                putJsonArray("scopeMetrics") {
                    addJsonObject {
                        putJsonObject("scope") { put("name", "com.cernauskas.shoppinglist") }
                        put("metrics", metrics)
                    }
                }
            }
        }
    }

    private fun attribute(key: String, value: String): JsonObject = buildJsonObject {
        put("key", key)
        putJsonObject("value") { put("stringValue", value) }
    }

    private fun attributesOf(series: Series): JsonArray = buildJsonArray {
        series.attributes.forEach { (key, value) -> add(attribute(key, value)) }
    }

    private fun sum(series: Series, value: Long, began: Long, now: Long): JsonObject =
        buildJsonObject {
            put("name", series.metric)
            put("unit", "1")
            putJsonObject("sum") {
                // 1 is DELTA. Cumulative would mean this app remembering totals across
                // restarts, which is a database for something a collector already does.
                put("aggregationTemporality", 1)
                put("isMonotonic", true)
                putJsonArray("dataPoints") {
                    addJsonObject {
                        put("attributes", attributesOf(series))
                        put("startTimeUnixNano", began.toString())
                        put("timeUnixNano", now.toString())
                        put("asInt", value.toString())
                    }
                }
            }
        }

    private fun gauge(series: Series, value: Long, now: Long): JsonObject = buildJsonObject {
        put("name", series.metric)
        put("unit", "1")
        putJsonObject("gauge") {
            putJsonArray("dataPoints") {
                addJsonObject {
                    put("attributes", attributesOf(series))
                    put("timeUnixNano", now.toString())
                    put("asInt", value.toString())
                }
            }
        }
    }

    private fun histogram(series: Series, buckets: Buckets, began: Long, now: Long): JsonObject =
        buildJsonObject {
            put("name", series.metric)
            put("unit", "ms")
            putJsonObject("histogram") {
                put("aggregationTemporality", 1)
                putJsonArray("dataPoints") {
                    addJsonObject {
                        put("attributes", attributesOf(series))
                        put("startTimeUnixNano", began.toString())
                        put("timeUnixNano", now.toString())
                        put("count", buckets.seen.toString())
                        put("sum", buckets.total)
                        putJsonArray("bucketCounts") {
                            buckets.counts.forEach { add(it.toString()) }
                        }
                        putJsonArray("explicitBounds") { BOUNDS.forEach { add(it) } }
                    }
                }
            }
        }

    private val JSON = "application/json".toMediaType()

    /**
     * How many series are waiting to be pushed.
     *
     * For the test that holds the standalone guard in place. "Nothing was collected" is
     * the whole promise a device answering for itself makes, and it is not checkable
     * from outside without this.
     */
    fun recorded(): Int = synchronized(lock) { counts.size + levels.size + durations.size }

    /** Everything counted so far, thrown away. For a test, and for somebody who has
     * just changed which collector this talks to. */
    fun forget() {
        synchronized(lock) {
            counts.clear()
            levels.clear()
            durations.clear()
        }
    }
}
