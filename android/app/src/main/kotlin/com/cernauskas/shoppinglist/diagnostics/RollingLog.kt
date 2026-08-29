package com.cernauskas.shoppinglist.diagnostics

import java.io.File
import java.io.IOException
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

/**
 * The log on disk, capped so it can never be the reason a phone runs out of room.
 *
 * Two files rather than one, and that is not an implementation detail: a single file
 * truncated when it fills throws away the beginning, which is where the thing that
 * started the trouble is. Rotating keeps a whole previous window, so the newest line and
 * the oldest kept line are always at least [capacityBytes] / 2 apart in the story.
 *
 * Nothing here throws. A device that will not hold a log file is a device with a worse
 * problem than a missing log, and taking the app down over it would be this feature
 * causing the outage it exists to explain.
 */
internal class RollingLog(
    private val directory: File,
    /** Both halves together. Small enough to attach to an email, which is where it goes. */
    private val capacityBytes: Long = 512L * 1024,
) {

    /** What is being written now. */
    private val current = File(directory, "diagnostics.log")

    /** The window before it. Overwritten on each rotation, so there are only ever two. */
    private val previous = File(directory, "diagnostics.1.log")

    private val half get() = capacityBytes / 2

    fun write(line: String) {
        try {
            directory.mkdirs()
            if (current.length() >= half) rotate()
            current.appendText(line + "\n")
        } catch (_: IOException) {
            // See the note above: there is nothing useful to do, and logcat still has it.
        } catch (_: SecurityException) {
        }
    }

    private fun rotate() {
        previous.delete()
        // Renamed rather than copied: a copy of a quarter of a megabyte on the logging
        // thread is a stall that shows up as jank in whatever recomposition happened to
        // be waiting behind it.
        if (!current.renameTo(previous)) current.delete()
    }

    /** How much is being kept, for a settings screen that should say. */
    fun sizeBytes(): Long = runCatching { current.length() + previous.length() }.getOrDefault(0L)

    fun forget() {
        runCatching { current.delete() }
        runCatching { previous.delete() }
    }

    /**
     * Both halves as one compressed file, oldest first.
     *
     * Compressed because a log is text and compresses to roughly a tenth, and because
     * an archive is what a share sheet offers to attach — a `.log` is offered as
     * "unknown file" by half the apps it is sent to.
     *
     * Written where a `FileProvider` can reach it and nowhere else. Null when there is
     * nothing to export, which the caller shows as a disabled button rather than an
     * empty archive somebody sends and waits on.
     */
    fun packUp(into: File): File? {
        val halves = listOf(previous, current).filter { it.exists() && it.length() > 0 }
        if (halves.isEmpty()) return null

        return try {
            into.mkdirs()
            val archive = File(into, "shopping-list-log.zip")
            ZipOutputStream(archive.outputStream().buffered()).use { zip ->
                // One entry rather than two: whoever reads this wants the story in
                // order, not two files to concatenate in the right one.
                zip.putNextEntry(ZipEntry("shopping-list.log"))
                halves.forEach { half -> half.inputStream().use { it.copyTo(zip) } }
                zip.closeEntry()
            }
            archive
        } catch (_: IOException) {
            null
        } catch (_: SecurityException) {
            null
        }
    }
}
