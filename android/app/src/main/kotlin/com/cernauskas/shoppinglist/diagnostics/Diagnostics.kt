package com.cernauskas.shoppinglist.diagnostics

import android.content.Context
import android.util.Log
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * What this app writes down about itself.
 *
 * Two places at once, because they are read by different people at different times.
 * Logcat is for whoever has the phone plugged in; the file beside it is for the bug
 * report that arrives a week later from somebody four hundred miles away, describing a
 * queue that would not drain. Neither is any use without the other.
 *
 * **The levels are not a volume knob, they are a promise.** `info`, `warn` and `error`
 * carry no personal data, and cannot — see the note on [Level], which is where the whole
 * argument is. `trace` and `debug` may carry anything including what is on the lists,
 * are off unless somebody turns them on, and say so when they are turned on.
 *
 * ## Calling this before it has started
 *
 * Every method is safe before [start]. A `Level` is a field on an object and the call
 * sites are all over the data layer, including in constructors that run before an
 * `Application` has done anything — so "not started yet" writes to logcat and drops the
 * file half, rather than throwing inside the thing being diagnosed.
 */
object Diagnostics {

    /**
     * One thread, and writes go on it.
     *
     * A log line costs a file append, and an append on the main thread is a stall
     * measured in whatever the storage feels like doing. Single so lines cannot
     * interleave, daemon so it is never the reason a process stays alive.
     */
    private val writing = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "diagnostics").apply { isDaemon = true }
    }

    @Volatile
    private var file: RollingLog? = null

    /** Where a packed-up archive goes. Under the cache directory, which is what the
     * `FileProvider` in the manifest is pointed at. */
    @Volatile
    private var exports: File? = null

    /**
     * Held rather than read per line.
     *
     * A line is written from every read and every write in the data layer, and asking
     * shared preferences each time is a disk read in front of a disk write. Set by
     * [start] and by the settings screen, which are the only two things that change it.
     */
    @Volatile
    private var level: Level = Level.default

    /** Called once from `Application.onCreate`, after [DiagnosticsSettings.start]. */
    fun start(context: Context) {
        level = DiagnosticsSettings.level
        startAt(File(context.filesDir, "diagnostics"), File(context.cacheDir, "diagnostics"))
    }

    /**
     * The same, at paths the caller names.
     *
     * For tests, which must not read or overwrite the log belonging to whatever else is
     * on the machine running them — the same lesson `LocalBackend.openAt` learned.
     */
    fun startAt(directory: File, exportsInto: File = directory) {
        file = RollingLog(directory)
        exports = exportsInto
    }

    /** How much goes anywhere, as things stand. */
    fun level(): Level = level

    /**
     * Changes it, and records the change at a level that is always on.
     *
     * The record matters: a log that starts mid-story with no note of why is a log
     * somebody reads as evidence that nothing happened before it.
     */
    fun setLevel(chosen: Level) {
        DiagnosticsSettings.level = chosen
        level = chosen
        warn(Event.LEVEL_CHANGED, Fact.of(Field.LEVEL, chosen))
    }

    // MARK: - Writing

    /**
     * Everything, including what is on the lists.
     *
     * The lambda is not evaluated unless this level is in force, which is the point:
     * building a string out of a list's contents costs nothing on a device where nobody
     * asked for it.
     */
    inline fun trace(event: Event, vararg facts: Fact, contents: () -> String) {
        if (admits(Level.TRACE)) revealing(Level.TRACE, event, facts, contents())
    }

    /**
     * What the app did, including what is on the lists.
     *
     * The other level a person has to ask for. See [trace]; the two differ in how much
     * rather than in what they may say.
     */
    inline fun debug(event: Event, vararg facts: Fact, contents: () -> String) {
        if (admits(Level.DEBUG)) revealing(Level.DEBUG, event, facts, contents())
    }

    /** What the app did, in counts, shapes, ids, durations and outcomes. */
    fun info(event: Event, vararg facts: Fact) = record(Level.INFO, event, facts, null)

    /** Something went wrong and the app carried on. */
    fun warn(event: Event, vararg facts: Fact) = record(Level.WARN, event, facts, null)

    /** Something went wrong and did not. */
    fun error(event: Event, vararg facts: Fact) = record(Level.ERROR, event, facts, null)

    // MARK: - The parts the inline functions above need to reach

    /**
     * Public so [trace] and [debug] can be inline, which is what keeps the lambda
     * unevaluated at the call site. Not part of the surface anybody should call.
     */
    fun admits(wanted: Level): Boolean = level.admits(wanted)

    /** The same, and the same reason. Named so a reader of a call site cannot mistake
     * it for something that redacts. */
    fun revealing(at: Level, event: Event, facts: Array<out Fact>, contents: String) =
        record(at, event, facts, contents)

    private fun record(at: Level, event: Event, facts: Array<out Fact>, contents: String?) {
        if (!level.admits(at)) return

        val line = buildString {
            append(stamp.get()!!.format(Date()))
            append(' ')
            append(at.label)
            append(' ')
            append(event.label)
            facts.forEach { append(' '); append(it) }
            // Last, and only ever present at trace and debug. A reader scanning the
            // left-hand columns of a file gets the shape of what happened without
            // reading anybody's shopping, which is also what a `grep` wants.
            if (contents != null) {
                append(" | ")
                append(contents)
            }
        }

        when (at) {
            Level.ERROR -> Log.e(TAG, line)
            Level.WARN -> Log.w(TAG, line)
            Level.INFO -> Log.i(TAG, line)
            Level.DEBUG -> Log.d(TAG, line)
            Level.TRACE -> Log.v(TAG, line)
        }

        val sink = file ?: return
        runCatching { writing.execute { sink.write(line) } }
    }

    // MARK: - Getting it off the device

    /** How much is being kept, for a settings screen that should say before it offers
     * to send it anywhere. */
    fun sizeBytes(): Long = file?.sizeBytes() ?: 0L

    /**
     * The log as one compressed file, or null when there is nothing to send.
     *
     * Blocks until everything already queued has been written, because the interesting
     * line is nearly always the last one and an export that races the writer misses it.
     */
    fun packUp(): File? {
        settle()
        val into = exports ?: return null
        val archive = file?.packUp(into) ?: return null
        info(Event.LOG_EXPORTED, Fact.of(Field.BYTES, archive.length()))
        settle()
        return archive
    }

    /** Throws the file away. Offered beside the export, because the other half of
     * "turn this on for a minute" is turning it off and leaving nothing behind. */
    fun forget() {
        settle()
        file?.forget()
    }

    /**
     * Waits for what has been queued to reach the disk.
     *
     * Used by the export and by the tests, which would otherwise assert against a file
     * the writer has not caught up with — a flake that only appears on a loaded machine.
     */
    fun settle() {
        val done = java.util.concurrent.CountDownLatch(1)
        runCatching { writing.execute { done.countDown() } }.onFailure { return }
        runCatching { done.await(2, TimeUnit.SECONDS) }
    }

    private const val TAG = "Shopping"

    /**
     * Per thread, because `SimpleDateFormat` is not safe to share and the alternative
     * is a lock on the path every log line takes.
     */
    private val stamp = ThreadLocal.withInitial {
        SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS", Locale.US)
    }
}
