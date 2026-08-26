package com.cernauskas.shoppinglist.data

import androidx.room.ColumnInfo
import androidx.room.Dao
import androidx.room.Entity
import androidx.room.Index
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.UUID

/**
 * Changes made on this device that the server has not been told about yet.
 *
 * The counterpart of [Cache]: that one holds what the server said, this one holds what
 * this device said back. Together they are what lets somebody cross things off in a
 * shop with no signal and find the list right when they come out.
 *
 * Unlike the cache, **this is not disposable**. A queued change exists nowhere else in
 * the world until it is sent, so the database it lives in may not be thrown away on a
 * schema change and its migrations are written by hand.
 *
 * Ordering is `sequence`, which is the row id and therefore monotonic: a device's own
 * changes are replayed in the order they were made, always. Ordering *between* devices
 * is a different question and is decided by `at` -- see docs/offline.md.
 */
@Entity(
    tableName = "operations",
    // The operation's own name, for the sync route to recognise a resend by. Unique so
    // a double-tap that somehow produced the same id cannot queue twice.
    indices = [Index(value = ["id"], unique = true), Index(value = ["list_id"])],
)
data class QueuedOperation(
    /** This device's order. The row id, so it can only ever count up. */
    @PrimaryKey(autoGenerate = true) val sequence: Long = 0,
    /** What this operation is called, everywhere. Minted here, sent as-is. */
    val id: String = UUID.randomUUID().toString(),
    val kind: String,
    @ColumnInfo(name = "list_id") val listId: Long,
    @ColumnInfo(name = "list_uuid") val listUuid: String,
    @ColumnInfo(name = "item_id") val itemId: Long,
    /** What the item is called once the server has ids to spare -- see docs/offline.md.
     * Carried now so the row does not need a migration when `POST /api/sync` lands. */
    @ColumnInfo(name = "item_uuid") val itemUuid: String,
    /** The arguments, as JSON. `{"done":true}` for [SET_DONE]. */
    val payload: String,
    /** When this device says it happened, in epoch seconds. Not yet sent -- the REST
     * routes stamp their own time. Recorded from the first day so the queue is not
     * lying about when it was written. */
    val at: Long,
) {
    companion object {
        const val SET_DONE = "set_done"
    }
}

@Dao
interface OutboxDao {
    @Insert(onConflict = OnConflictStrategy.ABORT)
    suspend fun add(operation: QueuedOperation)

    @Query("SELECT * FROM operations ORDER BY sequence")
    suspend fun all(): List<QueuedOperation>

    @Query("SELECT * FROM operations WHERE list_id = :listId ORDER BY sequence")
    suspend fun forList(listId: Long): List<QueuedOperation>

    @Query("DELETE FROM operations WHERE sequence = :sequence")
    suspend fun forget(sequence: Long)

    @Query("SELECT count(*) FROM operations")
    suspend fun waiting(): Int

    @Query("DELETE FROM operations")
    suspend fun forgetEverything()
}

/**
 * What happened when the queue was last drained.
 *
 * `sent` is how many reached the server, `waiting` how many are still here, and
 * `dropped` names the ones that will never land -- an item somebody deleted while this
 * device was away. Dropped work is worth telling somebody about: they watched
 * themselves do it. Refused work is *not* dropped; see [Outbox.drain].
 */
data class Drained(val sent: Int, val waiting: Int, val dropped: List<String> = emptyList())

/** The queue, in the app's own vocabulary. */
class Outbox(private val dao: OutboxDao) {

    /**
     * Queues a tick.
     *
     * The caller has already changed what is on screen. This is the promise that the
     * change will reach the server eventually, and the only place it exists until it
     * does.
     */
    suspend fun setDone(item: Item, list: ShoppingList, done: Boolean) = withContext(Dispatchers.IO) {
        dao.add(
            QueuedOperation(
                kind = QueuedOperation.SET_DONE,
                listId = list.id,
                listUuid = list.uuid,
                itemId = item.id,
                itemUuid = item.uuid,
                payload = if (done) """{"done":true}""" else """{"done":false}""",
                at = System.currentTimeMillis() / 1000,
            )
        )
    }

    suspend fun waiting(): Int = withContext(Dispatchers.IO) { runCatching { dao.waiting() }.getOrDefault(0) }

    /** What is queued against one list, oldest first. */
    suspend fun forList(listId: Long): List<QueuedOperation> =
        withContext(Dispatchers.IO) { runCatching { dao.forList(listId) }.getOrDefault(emptyList()) }

    suspend fun forgetEverything() = withContext(Dispatchers.IO) {
        runCatching { dao.forgetEverything() }
        Unit
    }

    /**
     * Sends what is queued, oldest first, and stops at the first thing it cannot send.
     *
     * **In order, and stopping.** The queue is this device's story of what happened,
     * and skipping past a stuck operation to send a later one would tell that story out
     * of order -- ticking something off after a delete that has not gone yet.
     *
     * What each outcome means:
     *
     * * **Sent** -- forgotten. The server has it.
     * * **No connection** -- kept, and the drain stops. This is the ordinary case and
     *   is not an error.
     * * **The row is gone** -- dropped, and named in the result. Delete is final (see
     *   docs/offline.md): a tick has nothing to land on and never will. The person is
     *   told, because they watched themselves tick it.
     * * **Refused** -- kept, and the drain stops. Somebody removed from a list keeps
     *   their queue: if they are invited back the work is still there, and nothing was
     *   quietly binned behind them.
     * * **Anything else** -- dropped. A malformed operation the server will refuse
     *   forever would block the queue behind it for good.
     */
    suspend fun drain(api: Api): Drained = withContext(Dispatchers.IO) {
        val queued = runCatching { dao.all() }.getOrDefault(emptyList())
        var sent = 0
        val dropped = mutableListOf<String>()

        for (operation in queued) {
            try {
                send(api, operation)
                dao.forget(operation.sequence)
                sent++
            } catch (_: ApiError.Transport) {
                break
            } catch (_: ApiError.NotFound) {
                dao.forget(operation.sequence)
                dropped += operation.describe()
            } catch (_: ApiError.Forbidden) {
                break
            } catch (_: ApiError.NotAdmitted) {
                break
            } catch (_: ApiError.Unauthorized) {
                break
            } catch (_: Exception) {
                dao.forget(operation.sequence)
                dropped += operation.describe()
            }
        }

        Drained(sent = sent, waiting = runCatching { dao.waiting() }.getOrDefault(0), dropped = dropped)
    }

    private suspend fun send(api: Api, operation: QueuedOperation) {
        when (operation.kind) {
            QueuedOperation.SET_DONE ->
                api.setDone(operation.itemId, operation.listId, operation.done)

            // A kind this build does not know is a downgrade, which is not a case that
            // can arise yet. Refused rather than skipped, so it is never silent.
            else -> throw ApiError.BadInput("Unknown queued operation: ${operation.kind}")
        }
    }
}

/** Whether a queued [QueuedOperation.SET_DONE] is a tick or an untick. */
val QueuedOperation.done: Boolean
    get() = payload.contains("\"done\":true")

private fun QueuedOperation.describe(): String = when (kind) {
    QueuedOperation.SET_DONE -> if (done) "crossing something off" else "putting something back"
    else -> "a change"
}
