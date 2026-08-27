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
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.double
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import java.time.Instant
import java.util.UUID

/**
 * Changes made on this device that the server has not been told about yet.
 *
 * The counterpart of [Cache]: that one holds what the server said, this one holds what
 * this device said back. Together they are what lets somebody shop with no signal and
 * find the list right when they come out.
 *
 * Unlike the cache, **this is not disposable**. A queued change exists nowhere else in
 * the world until it is sent, so the database it lives in is migrated by hand.
 *
 * Ordering is `sequence`, which is the row id and therefore monotonic: a device's own
 * changes are replayed in the order they were made, always. Ordering *between* devices
 * is a different question, decided by `at` -- see docs/offline.md.
 */
@Entity(
    tableName = "operations",
    // The operation's own name, which is what the sync route recognises a resend by.
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
    /** The row's id on the server, where there is one. Negative for a row this device
     * made offline, and used only for marking the screen -- what goes on the wire is
     * always the uuid. */
    @ColumnInfo(name = "item_id") val itemId: Long,
    /** What operations call the row. The only name that travels. */
    @ColumnInfo(name = "item_uuid") val itemUuid: String,
    /** The arguments, as JSON -- whatever the kind needs beyond the columns beside it. */
    val payload: String,
    /** When this device says it happened, in epoch seconds. Sent with the operation and
     * clamped forward by the server; behind is believed. */
    val at: Long,
) {
    companion object {
        /**
         * Make the list itself. Queued when a list is written down with nowhere to
         * send it — no signal, or no server at all.
         */
        const val MAKE_LIST = "make_list"
        const val ADD = "add"
        const val SET_DONE = "set_done"
        const val UPDATE = "update"
        const val DELETE = "delete"
        const val CLEAR_DONE = "clear_done"
        const val ATTACH_TAG = "attach_tag"
        const val DETACH_TAG = "detach_tag"
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

    @Query("DELETE FROM operations WHERE id = :id")
    suspend fun forget(id: String)

    @Query("SELECT count(*) FROM operations")
    suspend fun waiting(): Int

    @Query("DELETE FROM operations")
    suspend fun forgetEverything()
}

/**
 * What happened when the queue was last drained.
 *
 * `sent` reached the server. `waiting` is still here — either because there was no
 * connection, or because it was refused for want of access and is being kept in case
 * that changes. `lost` names what will never land, in words for a person: they watched
 * themselves do it, so it is worth saying.
 */
data class Drained(
    val sent: Int = 0,
    val waiting: Int = 0,
    val lost: List<String> = emptyList(),
    /** Something was refused. The one state of the three that interrupts. */
    val refused: Boolean = false,
    /**
     * Lists this device made offline, and the rows the server made for them, paired by
     * the `uuid` that never changed. The caller swaps this device's own numbering for
     * the server's — see `Cache.adopt`.
     */
    val adopted: List<Adopted> = emptyList(),
)

/** A list this device made offline, and the row the server made for it. */
data class Adopted(
    /** The name that never changed, and the only one both ends agree on. */
    val uuid: String,
    val real: ShoppingList,
)

/** The queue, in the app's own vocabulary. */
class Outbox(private val dao: OutboxDao) {

    private val json = Json { ignoreUnknownKeys = true }

    // ------------------------------------------------------------------ queueing

    /**
     * Says a list exists, under the name this device has been calling it by.
     *
     * Names no item, which is why the uuid is empty — the wire drops it, and the
     * list's own `uuid` is the only name this operation needs.
     */
    suspend fun makeList(list: ShoppingList) =
        queue(
            QueuedOperation.MAKE_LIST,
            "",
            list.id,
            list,
            buildJsonObject { put("name", list.name) },
        )

    /** Puts something on the list, under a name this device mints now. */
    suspend fun add(uuid: String, localId: Long, line: String, list: ShoppingList) =
        queue(QueuedOperation.ADD, uuid, localId, list, buildJsonObject { put("line", line) })

    suspend fun setDone(item: Item, list: ShoppingList, done: Boolean) =
        queue(
            QueuedOperation.SET_DONE,
            item.uuid,
            item.id,
            list,
            buildJsonObject { put("done", done) },
        )

    /**
     * Corrects what somebody typed, carrying what the row looked like at the time.
     *
     * `seen` is not decoration: it is what lets the server tell a plain rename from a
     * rename of something somebody else has edited meanwhile, and split rather than
     * overwrite. See docs/offline.md (5).
     */
    suspend fun update(
        item: Item,
        list: ShoppingList,
        name: String,
        amount: Double,
        unitId: Long?,
    ) = queue(
        QueuedOperation.UPDATE,
        item.uuid,
        item.id,
        list,
        buildJsonObject {
            put("name", name)
            put("amount", amount)
            unitId?.let { put("unit_id", it) }
            putJsonObject("seen") {
                put("name", item.name)
                put("amount", item.amount)
                item.unitId?.let { put("unit_id", it) }
            }
        },
    )

    /**
     * Files something under an aisle, or stops filing it there.
     *
     * The tag travels as an id, and that is only safe because the ids are agreed in
     * advance: `reference.json` is the same file the server's seed is checked against,
     * so a device that has never met a server still means aisle 5 by 5.
     */
    suspend fun tag(item: Item, list: ShoppingList, tagId: Long, attached: Boolean) =
        queue(
            if (attached) QueuedOperation.ATTACH_TAG else QueuedOperation.DETACH_TAG,
            item.uuid,
            item.id,
            list,
            buildJsonObject { put("tag_id", tagId) },
        )

    suspend fun delete(item: Item, list: ShoppingList) =
        queue(QueuedOperation.DELETE, item.uuid, item.id, list, buildJsonObject {})

    /**
     * Empties the trolley of exactly the rows this device could see.
     *
     * The ids are the point. "Clear everything that is done" replayed an hour later is
     * a different sentence, and would sweep away what somebody else ticked off
     * meanwhile -- docs/offline.md (4).
     */
    suspend fun clearDone(done: List<Item>, list: ShoppingList) = queue(
        QueuedOperation.CLEAR_DONE,
        // A sweep is about a list, not a row, so there is no item to name. The column
        // is not nullable and an empty string is the honest value for "no row".
        itemUuid = "",
        itemId = 0,
        list = list,
        payload = buildJsonObject {
            putJsonArray("items") { done.forEach { add(JsonPrimitive(it.uuid)) } }
        },
    )

    private suspend fun queue(
        kind: String,
        itemUuid: String,
        itemId: Long,
        list: ShoppingList,
        payload: JsonObject,
    ) = withContext(Dispatchers.IO) {
        dao.add(
            QueuedOperation(
                kind = kind,
                listId = list.id,
                listUuid = list.uuid,
                itemId = itemId,
                itemUuid = itemUuid,
                payload = payload.toString(),
                at = Instant.now().epochSecond,
            )
        )
    }

    // ------------------------------------------------------------------- reading

    suspend fun waiting(): Int =
        withContext(Dispatchers.IO) { runCatching { dao.waiting() }.getOrDefault(0) }

    /** What is queued against one list, oldest first. */
    suspend fun forList(listId: Long): List<QueuedOperation> =
        withContext(Dispatchers.IO) { runCatching { dao.forList(listId) }.getOrDefault(emptyList()) }

    suspend fun forgetEverything() = withContext(Dispatchers.IO) {
        runCatching { dao.forgetEverything() }
        Unit
    }

    // ------------------------------------------------------------------ sending

    /**
     * Sends the whole queue in one request and acts on what comes back.
     *
     * One request rather than one per operation: the batch is this device's story of
     * what it did, and the server replays it in order. Each operation gets its own
     * answer, so a refusal costs that change and no other.
     *
     * What each answer means:
     *
     * * **Applied**, or applied on an earlier send — forgotten. The server has it.
     * * **Refused because the list will not have you** — kept. If they are invited back
     *   the work is still here, and nothing was binned behind them. Reported, because
     *   this is the state that is worth interrupting somebody for.
     * * **Refused for any other reason** — forgotten, and named. A row somebody deleted
     *   is not coming back, and blocking the queue on it would cost every change behind
     *   it too.
     * * **No connection** — nothing is forgotten and nothing is said. This is the
     *   ordinary case.
     */
    suspend fun drain(api: Api): Drained = withContext(Dispatchers.IO) {
        val queued = runCatching { dao.all() }.getOrDefault(emptyList())
        if (queued.isEmpty()) return@withContext Drained()

        val answers = try {
            api.sync(queued.map(::onTheWire))
        } catch (_: ApiError.Transport) {
            return@withContext Drained(waiting = queued.size)
        } catch (_: ApiError.Unauthorized) {
            return@withContext Drained(waiting = queued.size)
        } catch (_: Exception) {
            // The route itself refused the request rather than the changes in it. Keep
            // everything: this is a fault to fix, not work to throw away.
            return@withContext Drained(waiting = queued.size)
        }

        var sent = 0
        val lost = mutableListOf<String>()
        var refused = false
        val adopted = mutableListOf<Adopted>()

        for (answer in answers) {
            when {
                answer.landed -> {
                    // A list this device made has just been given its real id.
                    // Collected rather than applied here, because the cache is the
                    // caller's and this type deliberately knows nothing about it.
                    val made = answer.list
                    val queuedRow = queued.firstOrNull { it.id == answer.id }
                    if (made != null && queuedRow != null) {
                        adopted += Adopted(queuedRow.listUuid, made)
                    }
                    dao.forget(answer.id)
                    sent++
                }
                answer.keepForLater -> refused = true
                else -> {
                    dao.forget(answer.id)
                    answer.lost?.let { lost += it }
                }
            }
        }

        Drained(
            sent = sent,
            waiting = runCatching { dao.waiting() }.getOrDefault(0),
            lost = lost,
            refused = refused,
            adopted = adopted,
        )
    }

    /**
     * One queued row as the route wants it.
     *
     * The stored payload is the operation's own arguments, so most of this is putting
     * the columns back beside them.
     */
    private fun onTheWire(operation: QueuedOperation): SyncOperation {
        val payload = runCatching { json.parseToJsonElement(operation.payload).jsonObject }
            .getOrDefault(JsonObject(emptyMap()))

        fun text(key: String) = payload[key]?.jsonPrimitive?.contentOrNull
        fun number(key: String) = payload[key]?.jsonPrimitive?.double
        fun flag(key: String) = payload[key]?.jsonPrimitive?.booleanOrNull

        return SyncOperation(
            id = operation.id,
            at = Instant.ofEpochSecond(operation.at).toString(),
            list = operation.listUuid,
            kind = operation.kind,
            item = operation.itemUuid.ifEmpty { null },
            items = payload["items"]?.jsonArray?.map { it.jsonPrimitive.content },
            line = text("line"),
            name = text("name"),
            amount = number("amount"),
            unitId = payload["unit_id"]?.jsonPrimitive?.content?.toLongOrNull(),
            seen = payload["seen"]?.jsonObject?.let { seen ->
                SeenOn(
                    name = seen["name"]?.jsonPrimitive?.content.orEmpty(),
                    amount = seen["amount"]?.jsonPrimitive?.double ?: 1.0,
                    unitId = seen["unit_id"]?.jsonPrimitive?.content?.toLongOrNull(),
                )
            },
            done = flag("done"),
            tagId = payload["tag_id"]?.jsonPrimitive?.content?.toLongOrNull(),
        )
    }
}

private val kotlinx.serialization.json.JsonPrimitive.contentOrNull: String?
    get() = if (this is kotlinx.serialization.json.JsonNull) null else content

// ---------------------------------------------------------------- reading a payload
//
// A queued operation's arguments live as JSON, because the five kinds want different
// things and five sets of nullable columns would be worse. These are the readings the
// screen needs to lay unsent work back over the server's answer -- see
// `ItemsViewModel.withUnsent`.

private val PAYLOAD = Json { ignoreUnknownKeys = true }

private fun QueuedOperation.field(key: String): kotlinx.serialization.json.JsonElement? =
    runCatching { PAYLOAD.parseToJsonElement(payload).jsonObject[key] }.getOrNull()

/** Whether a queued [QueuedOperation.SET_DONE] is a tick or an untick. */
val QueuedOperation.done: Boolean
    get() = field("done")?.jsonPrimitive?.booleanOrNull == true

/** The aisle a queued [QueuedOperation.ATTACH_TAG] or [QueuedOperation.DETACH_TAG] names. */
val QueuedOperation.tagId: Long?
    get() = field("tag_id")?.jsonPrimitive?.contentOrNull?.toLongOrNull()

/** The name a queued [QueuedOperation.UPDATE] gives the row. */
val QueuedOperation.editedName: String?
    get() = field("name")?.jsonPrimitive?.contentOrNull

/** The amount a queued [QueuedOperation.UPDATE] gives the row. */
val QueuedOperation.editedAmount: Double?
    get() = field("amount")?.jsonPrimitive?.doubleOrNull

/** The rows a queued [QueuedOperation.CLEAR_DONE] named. */
val QueuedOperation.sweptUuids: Set<String>
    get() = field("items")?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }?.toSet()
        ?: emptySet()
