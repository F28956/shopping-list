package com.cernauskas.shoppinglist.data

import android.content.Context
import androidx.room.ColumnInfo
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.Transaction
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import com.cernauskas.shoppinglist.diagnostics.Diagnostics
import com.cernauskas.shoppinglist.diagnostics.Event
import com.cernauskas.shoppinglist.diagnostics.Fact
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * What was on the screen the last time the server answered.
 *
 * This exists because of one bug: with no signal the app said "No lists yet", which
 * is the app claiming an emptiness it never verified. A person who has lists is told
 * they have none, and the only honest states are "here is what I last saw" and "I do
 * not know" — never "there is nothing".
 *
 * It is a cache and not yet a source of truth. Reads come from it when the server
 * cannot be reached; writes still go straight to the server and fail when it cannot.
 * The outbox that changes that is step 3 of docs/offline.md, and this table is
 * deliberately shaped so it can arrive without a migration of meaning: rows are keyed
 * on the server's `id` but carry the `uuid` the operations will name them by.
 */
@Entity(tableName = "lists")
data class CachedList(
    @PrimaryKey val id: Long,
    val uuid: String,
    val name: String,
    @ColumnInfo(name = "owner_id") val ownerId: Long,
    val role: String,
    /** Where it sat, so the cached screen is in the order the server sent and not in
     * whatever order the rows come back. */
    val position: Int,
)

@Entity(tableName = "items")
data class CachedItem(
    @PrimaryKey val id: Long,
    val uuid: String,
    @ColumnInfo(name = "list_id") val listId: Long,
    val name: String,
    val amount: Double,
    @ColumnInfo(name = "unit_id") val unitId: Long?,
    @ColumnInfo(name = "done_at") val doneAt: String?,
    /** Comma-separated tag ids. A join table for a cache of a page would be three
     * queries to rebuild something the server sends as one array. */
    @ColumnInfo(name = "tag_ids") val tagIds: String,
    val position: Int,
)

/** Units and tags, cached for the same reason: a list read offline should still be
 * measured and filed rather than a column of bare names. `list_id` is null for units,
 * which are global, and set for tags, whose order is per person and per list. */
@Entity(tableName = "reference", primaryKeys = ["kind", "list_id", "id"])
data class CachedReference(
    val kind: String,
    @ColumnInfo(name = "list_id") val listId: Long,
    val id: Long,
    val name: String,
    val emoji: String?,
    val position: Int,
)

/**
 * What a list has taught the box, as this device last heard it.
 *
 * Keyed by the pair, because the memory belongs to the *list* rather than to whoever is
 * signed in — the server moved it there so a household shares one. The same name on two
 * lists is two habits: milk in pints at home, milk in litres for the office.
 *
 * Cached for the same reason the rows are: without it a phone in a shop offers no
 * suggestions at all, and a re-typed line arrives bare — no amount, no unit, nothing
 * filed. On a device answering for itself this is not used; the device's own server has
 * the real thing.
 */
@Entity(tableName = "history", primaryKeys = ["list_id", "name"])
data class CachedRemembered(
    @ColumnInfo(name = "list_id") val listId: Long,
    /** Trimmed and lowercased, so `Milk` and `milk ` are one memory. */
    val name: String,
    /** The spelling last used, for showing back. */
    val display: String,
    @ColumnInfo(name = "unit_id") val unitId: Long?,
    val amount: Double?,
    /** Comma-separated, as the items table does it and for the same reason. */
    @ColumnInfo(name = "tag_ids") val tagIds: String,
    val uses: Long,
    @ColumnInfo(name = "last_used_at") val lastUsedAt: Long,
)

@Dao
interface CacheDao {
    @Query("SELECT * FROM lists ORDER BY position")
    suspend fun lists(): List<CachedList>

    @Query("DELETE FROM lists")
    suspend fun forgetLists()

    /**
     * Everything the server has heard of.
     *
     * Lists this device made and has not managed to send have negative ids and keep
     * their rows: the server has never heard of them, so it cannot mention them, and
     * deleting everything it did not mention would take somebody's shopping away for
     * the crime of having been written down offline.
     */
    @Query("DELETE FROM lists WHERE id >= 0")
    suspend fun forgetKnownLists()

    /**
     * The other half: lists this device made and no server has heard of.
     *
     * Only ever called at the moment a device hands itself to a server, where these are
     * the photograph the takeover left behind -- see [Cache.forgetLocalLists]. Anywhere
     * else this would be deleting somebody's shopping for the crime of having been
     * written down offline.
     */
    @Query("DELETE FROM lists WHERE id < 0")
    suspend fun forgetLocalLists()

    @Query("SELECT min(id) FROM lists")
    suspend fun lowestListId(): Long?

    @Query("SELECT count(*) FROM lists")
    suspend fun listCount(): Int

    /**
     * The lowest id any row has, so the next one this device mints can count down from
     * it.
     *
     * Counted rather than taken from the clock. A millisecond timestamp is unique until
     * two adds land in the same millisecond, and then it is a primary key collision that
     * rolls back the whole write -- silently, if nobody is looking.
     */
    @Query("SELECT MIN(id) FROM items")
    suspend fun lowestItemId(): Long?

    @Query("SELECT * FROM history WHERE list_id = :listId")
    suspend fun remembered(listId: Long): List<CachedRemembered>

    @Query("DELETE FROM history WHERE list_id = :listId")
    suspend fun forgetRemembered(listId: Long)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putRemembered(rows: List<CachedRemembered>)

    @Transaction
    suspend fun replaceRemembered(listId: Long, rows: List<CachedRemembered>) {
        forgetRemembered(listId)
        putRemembered(rows)
    }

    @Query("UPDATE lists SET id = :real, owner_id = :owner WHERE id = :local")
    suspend fun renumberList(local: Long, real: Long, owner: Long)

    @Query("UPDATE items SET list_id = :real WHERE list_id = :local")
    suspend fun renumberItems(local: Long, real: Long)

    @Query("UPDATE reference SET list_id = :real WHERE list_id = :local")
    suspend fun renumberReference(local: Long, real: Long)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putList(row: CachedList)

    /**
     * Gives a locally-made list the id the server gave it.
     *
     * Everything keyed by the old id moves with it. Missing one of those would leave
     * rows pointing at a list id that no longer exists, which reads on screen as a
     * list that lost its items the moment it was first synced.
     *
     * The `uuid` does not change and never has — it is what the server was told, and
     * what every queued operation names. Only this device's own numbering moves.
     */
    @Transaction
    suspend fun adoptList(local: Long, real: Long, owner: Long) {
        renumberList(local, real, owner)
        renumberItems(local, real)
        renumberReference(local, real)
    }

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putLists(rows: List<CachedList>)

    /**
     * Replaces the cached lists wholesale.
     *
     * In one transaction, because the alternative — delete, then insert — has a moment
     * where the cache says there are no lists, and a read landing in that moment is
     * the very bug this table exists to fix.
     */
    @Transaction
    suspend fun replaceLists(rows: List<CachedList>) {
        forgetKnownLists()
        putLists(rows)
    }

    @Query("SELECT * FROM items WHERE list_id = :listId ORDER BY position")
    suspend fun items(listId: Long): List<CachedItem>

    @Query("DELETE FROM items WHERE list_id = :listId")
    suspend fun forgetItems(listId: Long)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putItems(rows: List<CachedItem>)

    @Transaction
    suspend fun replaceItems(listId: Long, rows: List<CachedItem>) {
        forgetItems(listId)
        putItems(rows)
    }

    @Query("SELECT * FROM reference WHERE kind = :kind AND list_id = :listId ORDER BY position")
    suspend fun reference(kind: String, listId: Long): List<CachedReference>

    @Query("DELETE FROM reference WHERE kind = :kind AND list_id = :listId")
    suspend fun forgetReference(kind: String, listId: Long)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putReference(rows: List<CachedReference>)

    @Transaction
    suspend fun replaceReference(kind: String, listId: Long, rows: List<CachedReference>) {
        forgetReference(kind, listId)
        putReference(rows)
    }

    /** Everything this person had. Called on sign-out: the next person to sign in on
     * this phone must not be shown somebody else's shopping. */
    @Transaction
    suspend fun forgetEverything() {
        forgetLists()
        forgetAllItems()
        forgetAllReference()
    }

    @Query("DELETE FROM items")
    suspend fun forgetAllItems()

    @Query("DELETE FROM reference")
    suspend fun forgetAllReference()
}

@Database(
    entities = [
        CachedList::class,
        CachedItem::class,
        CachedReference::class,
        CachedRemembered::class,
        QueuedOperation::class,
    ],
    version = 3,
    exportSchema = true,
)
abstract class CacheDatabase : RoomDatabase() {
    abstract fun dao(): CacheDao
    abstract fun outbox(): OutboxDao
}

/**
 * Adds the outbox.
 *
 * Written by hand rather than left to a destructive fallback, and that is the whole
 * point of it: a queued change exists nowhere else in the world, so upgrading the app
 * may not be a way to lose one. The cached rows beside it *are* disposable, but they
 * share a file with something that is not, so the file is migrated properly.
 *
 * Kept in step with `app/schemas/…/2.json`, which is committed for exactly this reason.
 */
/**
 * Adds what each list has taught the box.
 *
 * Written by hand for the same reason as the outbox beside it, though for a weaker one:
 * this table *is* disposable -- the server can hand it back -- but it shares a file with
 * a queue that is not, so the file is migrated rather than dropped.
 */
val ADD_THE_MEMORY = object : Migration(2, 3) {
    override fun migrate(connection: SupportSQLiteDatabase) {
        connection.execSQL(
            """
            CREATE TABLE IF NOT EXISTS `history` (
                `list_id` INTEGER NOT NULL,
                `name` TEXT NOT NULL,
                `display` TEXT NOT NULL,
                `unit_id` INTEGER,
                `amount` REAL,
                `tag_ids` TEXT NOT NULL,
                `uses` INTEGER NOT NULL,
                `last_used_at` INTEGER NOT NULL,
                PRIMARY KEY(`list_id`, `name`)
            )
            """.trimIndent()
        )
    }
}

val ADD_THE_OUTBOX = object : Migration(1, 2) {
    override fun migrate(connection: SupportSQLiteDatabase) {
        connection.execSQL(
            """
            CREATE TABLE IF NOT EXISTS `operations` (
                `sequence` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                `id` TEXT NOT NULL,
                `kind` TEXT NOT NULL,
                `list_id` INTEGER NOT NULL,
                `list_uuid` TEXT NOT NULL,
                `item_id` INTEGER NOT NULL,
                `item_uuid` TEXT NOT NULL,
                `payload` TEXT NOT NULL,
                `at` INTEGER NOT NULL
            )
            """.trimIndent()
        )
        connection.execSQL(
            "CREATE UNIQUE INDEX IF NOT EXISTS `index_operations_id` ON `operations` (`id`)"
        )
        connection.execSQL(
            "CREATE INDEX IF NOT EXISTS `index_operations_list_id` ON `operations` (`list_id`)"
        )
    }
}

/**
 * The cache, in the app's own vocabulary.
 *
 * The view models talk in [ShoppingList] and [Item] and know nothing about Room; this
 * is where the two shapes meet. Every method is safe to call with no connection and
 * none of them throw: a cache that fails is a cache that is missing, and a screen
 * asking for the last thing it saw has nothing useful to do with an exception.
 */
class Cache(
    context: Context,
    /**
     * Whether there is a server to send to. Passed to the [Outbox], which is what
     * actually decides -- see the guard in `Outbox.queue`.
     *
     * Injected so a test can say which it means rather than reaching for storage that
     * the rest of the suite is also reading. It was global before, and two tests could
     * not disagree about it.
     */
    private val sending: () -> Boolean = { !ServerDirectory.isOnDeviceOnly },
    /** In memory, for a test that should neither read nor overwrite a real device's. */
    inMemory: Boolean = false,
) {

    /** Kept for the bundled reference data, which is read out of the assets. */
    private val assets = context.applicationContext

    private val db = (
        if (inMemory) {
            Room.inMemoryDatabaseBuilder(context.applicationContext, CacheDatabase::class.java)
        } else {
            Room.databaseBuilder(
                context.applicationContext,
                CacheDatabase::class.java,
                "cache.db",
            )
        }
        )
        // Migrated, not thrown away. The cached rows in here could be discarded on a
        // schema change -- they are a copy of what the server holds -- but the outbox
        // beside them holds changes that exist nowhere else, and the two share a file.
        // So the file is migrated properly and nobody loses a shop's worth of ticks to
        // an app update.
        .addMigrations(ADD_THE_OUTBOX, ADD_THE_MEMORY)
        .build()

    private val dao = db.dao()

    /** The queue that lives in the same file — see [Outbox]. */
    val outbox = Outbox(db.outbox(), sending)

    /**
     * Writes down the units and aisles a device with no server would otherwise never
     * have — see [Reference].
     *
     * Written into the cache rather than only handed back, so the next screen finds
     * them without asking, and so a device that later gains a server simply overwrites
     * them with that server's answer.
     *
     * Does nothing if there is already something there. A server's answer is the
     * authority and must not be replaced by the bundled copy on the next cold start.
     */
    suspend fun seedReference(list: ShoppingList): Pair<List<Unit>, List<Tag>> {
        val units = Reference.units(assets)
        val tags = Reference.tags(assets)
        if (this.units().isEmpty()) rememberUnits(units)
        if (this.tags(list).isEmpty()) rememberTags(list, tags)
        return units to tags
    }

    suspend fun lists(): List<ShoppingList> = read {
        dao.lists().map {
            ShoppingList(
                id = it.id,
                uuid = it.uuid,
                name = it.name,
                ownerId = it.ownerId,
                role = Role.entries.firstOrNull { role -> role.name == it.role } ?: Role.VIEWER,
            )
        }
    }

    suspend fun rememberLists(lists: List<ShoppingList>) = write {
        dao.replaceLists(
            lists.mapIndexed { at, list ->
                CachedList(
                    id = list.id,
                    uuid = list.uuid,
                    name = list.name,
                    ownerId = list.ownerId,
                    role = list.role.name,
                    position = at,
                )
            }
        )
    }

    /**
     * Makes a list here, with no server involved.
     *
     * The id is negative and minted locally, which is the same trick items already use
     * for rows created offline: it is a key for this device's own tables and never goes
     * on the wire, where the `uuid` is the only name. When the server finally hears
     * about it, [adopt] swaps the one for the other.
     *
     * Counting down from the lowest already used, so two lists made in the same second
     * cannot collide.
     */
    /** What this list has taught the box, as last heard from the server. */
    suspend fun rememberedFor(list: ShoppingList): List<RememberedEntry> = read {
        dao.remembered(list.id).map {
            RememberedEntry(
                name = it.name,
                display = it.display,
                unitId = it.unitId,
                amount = it.amount,
                tags = it.tagIds.split(",").filter(String::isNotBlank).map(String::toLong),
                uses = it.uses,
                lastUsedAt = it.lastUsedAt,
            )
        }
    }

    /**
     * Takes the server's memory as this device's copy.
     *
     * A replace, like the item cache: what the server holds is the household's memory
     * and this is a copy of it. Merging would mean deciding whose count and whose
     * last-used wins, which is a conflict rule for something that has an authority.
     */
    suspend fun rememberHistory(list: ShoppingList, entries: List<RememberedEntry>) = write {
        dao.replaceRemembered(
            list.id,
            entries.map {
                CachedRemembered(
                    listId = list.id,
                    name = it.name,
                    display = it.display,
                    unitId = it.unitId,
                    amount = it.amount,
                    tagIds = it.tags.joinToString(","),
                    uses = it.uses,
                    lastUsedAt = it.lastUsedAt,
                )
            },
        )
    }

    /** See [CacheDao.lowestItemId]. Zero when nothing has been written down yet. */
    suspend fun lowestItemId(): Long = withContext(Dispatchers.IO) {
        runCatching { dao.lowestItemId() ?: 0L }.getOrDefault(0L)
    }

    /**
     * Takes lists and their rows in as this device's own, keeping the names a server
     * will be told.
     *
     * The other end of the handover. `CachingBackend.handOverIfNeeded` walks this cache
     * for lists no server has heard of and queues them; this is how the lists on a
     * device that has been answering for *itself* get in here to be walked. Without it,
     * adopting a server shows an empty account with a year of shopping still on disk:
     * everything lives in `device.sqlite`, the queue is built from this, and nothing
     * joins the two.
     *
     * The uuids come in rather than being minted, unlike [makeListHere]. They are what
     * every queued operation names and what the server records, and a new one here would
     * make the same list twice the first time the device and the server ever met. The
     * ids are local because that is precisely what "no server has heard of this" is
     * written as.
     */
    suspend fun takeIn(incoming: List<Pair<ShoppingList, List<Item>>>) {
        if (incoming.isEmpty()) return

        write {
            var nextList = minOf(dao.lowestListId() ?: 0L, 0L) - 1
            var nextItem = minOf(dao.lowestItemId() ?: 0L, 0L)
            var position = dao.listCount()

            for ((list, items) in incoming) {
                // Already here, so this has run before. Left alone rather than written
                // twice: the same list under two ids is the same shopping told to the
                // server twice.
                if (dao.lists().any { it.uuid == list.uuid }) continue

                dao.putList(
                    CachedList(
                        id = nextList,
                        uuid = list.uuid,
                        name = list.name,
                        ownerId = list.ownerId,
                        role = Role.OWNER.name,
                        position = position,
                    )
                )

                dao.putItems(
                    items.mapIndexed { at, item ->
                        nextItem -= 1
                        CachedItem(
                            id = nextItem,
                            uuid = item.uuid,
                            listId = nextList,
                            name = item.name,
                            amount = item.amount,
                            unitId = item.unitId,
                            doneAt = item.doneAt,
                            tagIds = item.tagIds.joinToString(","),
                            position = at,
                        )
                    }
                )

                nextList -= 1
                position += 1
            }
        }
    }

    /**
     * Drops the lists this device kept from before its own server took over.
     *
     * `readyForUse` copies the cache into `device.sqlite` and leaves this exactly as it
     * was, deliberately, so the move stays reversible. Once that has happened these rows
     * are a photograph of a moment: every edit since went to `device.sqlite`, and the
     * two copies do not even share uuids because the migration mints new ones for the
     * lists. Handing the device to a server without dropping them would queue both, and
     * the server would be told about the same shopping twice under two different names.
     */
    suspend fun forgetLocalLists() = write {
        for (list in dao.lists().filter { it.id < 0 }) {
            dao.forgetItems(list.id)
        }
        dao.forgetLocalLists()
    }

    suspend fun makeListHere(name: String, ownedBy: Long): ShoppingList {
        val list = ShoppingList(
            id = minOf(dao.lowestListId() ?: 0L, 0L) - 1,
            uuid = java.util.UUID.randomUUID().toString(),
            name = name,
            ownerId = ownedBy,
            role = Role.OWNER,
        )

        write {
            dao.putList(
                CachedList(
                    id = list.id,
                    uuid = list.uuid,
                    name = list.name,
                    ownerId = list.ownerId,
                    role = list.role.name,
                    position = dao.listCount(),
                )
            )
        }

        return list
    }

    /** Gives a locally-made list the id the server gave it. See [CacheDao.adoptList]. */
    suspend fun adopt(local: ShoppingList, real: ShoppingList) = write {
        if (isLocal(local) && !isLocal(real)) {
            dao.adoptList(local.id, real.id, real.ownerId)
        }
    }

    suspend fun items(list: ShoppingList): List<Item> = read {
        dao.items(list.id).map {
            Item(
                id = it.id,
                uuid = it.uuid,
                name = it.name,
                amount = it.amount,
                unitId = it.unitId,
                doneAt = it.doneAt,
                tagIds = it.tagIds.split(',').filter(String::isNotEmpty).map(String::toLong),
            )
        }
    }

    suspend fun rememberItems(list: ShoppingList, items: List<Item>) = write {
        dao.replaceItems(
            list.id,
            items.mapIndexed { at, item ->
                CachedItem(
                    id = item.id,
                    uuid = item.uuid,
                    listId = list.id,
                    name = item.name,
                    amount = item.amount,
                    unitId = item.unitId,
                    doneAt = item.doneAt,
                    tagIds = item.tagIds.joinToString(","),
                    position = at,
                )
            }
        )
    }

    suspend fun units(): List<Unit> = read {
        dao.reference(UNITS, GLOBAL).map { Unit(id = it.id, name = it.name) }
    }

    suspend fun rememberUnits(units: List<Unit>) = write {
        dao.replaceReference(
            UNITS,
            GLOBAL,
            units.mapIndexed { at, unit ->
                CachedReference(UNITS, GLOBAL, unit.id, unit.name, null, at)
            },
        )
    }

    suspend fun tags(list: ShoppingList): List<Tag> = read {
        dao.reference(TAGS, list.id).mapIndexed { at, row ->
            // `sortOrder` is the server's own column and is not what decides the order
            // here -- `position` already holds the order this person resolved. It is
            // filled from the position so nothing downstream reads a zero as a tie.
            Tag(id = row.id, name = row.name, emoji = row.emoji, sortOrder = at.toLong())
        }
    }

    suspend fun rememberTags(list: ShoppingList, tags: List<Tag>) = write {
        dao.replaceReference(
            TAGS,
            list.id,
            tags.mapIndexed { at, tag ->
                CachedReference(TAGS, list.id, tag.id, tag.name, tag.emoji, at)
            },
        )
    }

    /**
     * Called when somebody signs out.
     *
     * The queue goes too. Its contents are changes to somebody else's lists, made by
     * somebody who is no longer here, and sending them under the next person's token
     * would be a stranger writing to a stranger's shopping.
     */
    suspend fun forgetEverything() {
        write { dao.forgetEverything() }
        outbox.forgetEverything()
    }

    private suspend fun <T> read(work: suspend () -> List<T>): List<T> =
        withContext(Dispatchers.IO) {
            try {
                work()
            } catch (_: Exception) {
                // A cache that cannot be read is a cache that holds nothing, and the
                // caller already has an answer for that.
                emptyList()
            }
        }

    private suspend fun write(work: suspend () -> kotlin.Unit) {
        withContext(Dispatchers.IO) {
            try {
                work()
            } catch (problem: Exception) {
                // Nothing on the screen depends on this having happened, which is
                // exactly why it is written down. A cache write that fails every time
                // -- a full disk, a constraint the migrations left behind -- shows up
                // as an app that forgets what it saw the moment it goes offline, and
                // there is no other trace of it anywhere: the read path answers
                // "nothing cached" and is telling the truth.
                Diagnostics.warn(Event.CACHE_WRITE_FAILED, Fact.failure(problem))
            }
        }
    }

    companion object {
        /**
         * A cache that neither reads nor overwrites the one on the device running the
         * tests.
         *
         * `sending` says which mode the test means. Naming it at the call site is the
         * point: "nothing was queued" and "everything was queued" are both correct
         * answers, and which one a test expects depends on a question it should have to
         * answer out loud.
         */
        fun inMemory(context: Context, sending: () -> Boolean = { true }): Cache =
            Cache(context, sending, inMemory = true)

        /**
         * Whether this list exists only here — see [makeListHere]. Public because the
         * screens ask; the rest of this object is not.
         */
        fun isLocal(list: ShoppingList): Boolean = list.id < 0

        private const val UNITS = "unit"
        private const val TAGS = "tag"

        /** `list_id` for rows that belong to no list. Units are the same everywhere,
         * so they are cached once rather than once per list. */
        private const val GLOBAL = 0L
    }
}
