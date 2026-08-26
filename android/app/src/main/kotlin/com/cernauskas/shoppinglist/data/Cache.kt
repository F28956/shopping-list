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

@Dao
interface CacheDao {
    @Query("SELECT * FROM lists ORDER BY position")
    suspend fun lists(): List<CachedList>

    @Query("DELETE FROM lists")
    suspend fun forgetLists()

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
        forgetLists()
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
    entities = [CachedList::class, CachedItem::class, CachedReference::class],
    version = 1,
    exportSchema = true,
)
abstract class CacheDatabase : RoomDatabase() {
    abstract fun dao(): CacheDao
}

/**
 * The cache, in the app's own vocabulary.
 *
 * The view models talk in [ShoppingList] and [Item] and know nothing about Room; this
 * is where the two shapes meet. Every method is safe to call with no connection and
 * none of them throw: a cache that fails is a cache that is missing, and a screen
 * asking for the last thing it saw has nothing useful to do with an exception.
 */
class Cache(context: Context) {

    private val db = Room.databaseBuilder(
        context.applicationContext,
        CacheDatabase::class.java,
        "cache.db",
    )
        // The cache is a copy of what the server holds, so a schema change may throw
        // it away rather than migrate it. The only cost is one load with no signal
        // after an upgrade; the outbox in step 3 is not disposable this way and will
        // need real migrations when it lands.
        .fallbackToDestructiveMigration()
        .build()

    private val dao = db.dao()

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

    /** Called when somebody signs out. */
    suspend fun forgetEverything() = write { dao.forgetEverything() }

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
            } catch (_: Exception) {
                // Nothing on the screen depends on this having happened.
            }
        }
    }

    private companion object {
        const val UNITS = "unit"
        const val TAGS = "tag"

        /** `list_id` for rows that belong to no list. Units are the same everywhere,
         * so they are cached once rather than once per list. */
        const val GLOBAL = 0L
    }
}
