package com.cernauskas.shoppinglist.data

import android.content.Context
import android.util.Log
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * The units and tags every list can rely on being there.
 *
 * Normally these come from the server, which is the authority on them. But they are the
 * one part of this application's data that is the *same everywhere* -- seeded by
 * migration, writable only by the process itself, belonging to no user -- so a device
 * with no server can have them too, and must: without units an item has no measure, and
 * without tags a list has no aisles.
 *
 * The file is `reference/reference.json`, shared with the server and with the Apple
 * apps, and guarded by `domain::reference::the_seed_and_the_file_agree`, which fails if
 * it and the migrations ever disagree. Gradle copies it into assets at build time, so
 * there is no second copy in this tree to drift.
 *
 * **The ids are the point, not just the names.** An item added here carries its
 * `unit_id` when a server finally hears about it, so the numbers have to be the
 * server's numbers.
 */
object Reference {

    private val json = Json { ignoreUnknownKeys = true }

    @Serializable
    private data class File(val units: List<Unit> = emptyList(), val tags: List<Tag> = emptyList())

    private var loaded: File? = null

    /** Read once. It is a few kilobytes and it never changes within a run. */
    private fun read(context: Context): File = loaded ?: try {
        json.decodeFromString<File>(
            context.assets.open("reference.json").bufferedReader().use { it.readText() }
        ).also { loaded = it }
    } catch (e: Exception) {
        // A build that forgot the asset, or a file that will not parse. Empty rather
        // than a crash: the app still works with a server, which is the case that
        // would notice.
        Log.e("Reference", "reference.json did not load; there will be no units or aisles", e)
        File()
    }

    fun units(context: Context): List<Unit> = read(context).units

    /**
     * Which units may be written with no number in front of them — `pint milk`.
     *
     * Read from here rather than from the cache, which has no column for it. That is
     * not a gap to be migrated: `bare` is a fact about the shipped vocabulary, seeded by
     * the same migration on every server, and this file is where the shipped vocabulary
     * lives. A cached copy of it would be a second answer to a question that has one.
     */
    fun bareUnitIds(context: Context): Set<Long> =
        read(context).units.filter { it.bare }.map { it.id }.toSet()

    fun tags(context: Context): List<Tag> = read(context).tags
}
