package com.cernauskas.shoppinglist.data

import com.cernauskas.shoppinglist.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import okhttp3.sse.EventSources
import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** What went wrong, in terms a screen can answer. */
sealed class ApiError(message: String) : Exception(message) {
    /** The server did not accept the token. Usually means signed out. */
    data object Unauthorized : ApiError("Signed out. Sign in again.")
    data object NotFound : ApiError("That is not there any more.")
    data object Forbidden : ApiError("You can look at this list but not change it.")

    /**
     * This account may not use this server at all.
     *
     * Shares 403 with [Forbidden] and is a different thing entirely: that one is a
     * sentence about a list, this one is a sentence about the account, and asking
     * again will not change it. Told apart by the `reason` in the body, because the
     * status cannot tell them apart -- and when nothing did, somebody signing in with
     * an unlisted address was told they could read a list they did not have.
     */
    data object NotAdmitted : ApiError("This account is not allowed to use this server.")
    data class BadInput(val what: String) : ApiError(what)
    data class Server(val code: Int) : ApiError("The server had a problem ($code).")
    data class Transport(val reason: Throwable) :
        ApiError(reason.message ?: "Could not reach the server.")
}

/**
 * The API, as this app uses it.
 *
 * Every call carries a bearer token and nothing else: the API never reads cookies,
 * which is what lets it share an origin with the browser safely.
 */
class Api(
    private val baseUrl: String = BuildConfig.API_BASE_URL,
    private val token: suspend () -> String?,
    /**
     * Whether somebody is signed in on this device, whether or not there is a token to
     * hand right now.
     *
     * The two are different questions offline. Google cannot be asked for a token
     * without a connection, and treating that as "signed out" would put the sign-in
     * screen in front of somebody whose own list is sitting on the phone -- so a
     * missing token with a remembered session is reported as a transport failure,
     * which is what it is.
     */
    private val remembered: () -> Boolean = { false },
) {
    private val json = Json { ignoreUnknownKeys = true }

    private val client = OkHttpClient.Builder()
        // Long, because the event stream is meant to stay open and say nothing for
        // long stretches -- but not infinite, which is what it was.
        //
        // The server sends a keep-alive every fifteen seconds. With no timeout at all,
        // a connection that dies without a FIN -- a phone going into a tunnel, or an
        // emulator switched to airplane mode -- left `collect` waiting for ever. The
        // screen never learned it was offline, and worse, never learned it was back:
        // the reconnect that triggers a reload, and the reload that empties the
        // outbox, both hang off that stream ending.
        //
        // Three missed keep-alives is dead enough to act on.
        .readTimeout(45, TimeUnit.SECONDS)
        .build()

    /** The service's own ceiling, and what the browser asks for. Asking for less is
     * how a screen comes to show a prefix of a list without saying so. */
    private val pageLimit = 500

    // ---------------------------------------------------------------- reading

    suspend fun lists(): Listing<ShoppingList> {
        val page: Page<ShoppingList> =
            get("/api/lists?order_by=updated_at&direction=descending&size=$pageLimit")
        return Listing(page.items, page.total, page.hasMore)
    }

    suspend fun items(list: ShoppingList): Listing<Item> {
        // Outstanding first, then what is already in the trolley — the same order
        // every other client asks for, so the four do not show a list differently.
        val page: Page<Item> = get(
            "/api/lists/${list.id}/items?order_by=done_at&direction=ascending&size=$pageLimit"
        )
        return Listing(page.items, page.total, page.hasMore)
    }

    suspend fun units(): List<Unit> =
        get<Page<Unit>>("/api/units?order_by=name&size=$pageLimit").items

    /**
     * Every tag, in the order that decides where this list's items sit.
     *
     * Not `/api/tags`, which is one global opinion. This is resolved per person and
     * per list by the service, so grouping reads position in this answer.
     */
    suspend fun tagsOrderedFor(list: ShoppingList): List<Tag> =
        get("/api/lists/${list.id}/tag-order")

    suspend fun setTagOrder(tags: List<Tag>, list: ShoppingList) {
        send("PUT", "/api/lists/${list.id}/tag-order", """{"tag_ids":${tags.map { it.id }}}""")
    }

    suspend fun tagsOn(item: Item, list: ShoppingList): List<Tag> =
        get("/api/lists/${list.id}/items/${item.id}/tags")

    /** What this list buys that matches what has been typed. Matched and ranked by
     * the service, so this shows what it is given. */
    suspend fun suggestions(typed: String, list: ShoppingList): List<String> =
        get("/api/lists/${list.id}/history?q=${typed.urlEncoded()}")

    // ---------------------------------------------------------------- writing

    /** Sent under `line`, not `name`: `name` is taken literally and `line` is read the
     * way a person means it. The parsing is the server's, so "2 kg apples" means the
     * same here as in the browser. */
    suspend fun add(line: String, list: ShoppingList) {
        send("POST", "/api/lists/${list.id}/items", """{"line":${line.asJson()}}""")
    }

    suspend fun setDone(item: Item, list: ShoppingList, done: Boolean) =
        setDone(item.id, list.id, done)

    /**
     * The same call, by id.
     *
     * What the outbox replays holds ids rather than rows: the row it was made against
     * may have changed three times since, and the operation is about the item, not
     * about the copy of it that happened to be on screen.
     */
    suspend fun setDone(itemId: Long, listId: Long, done: Boolean) {
        val path = "/api/lists/$listId/items/$itemId/done"
        send(if (done) "POST" else "DELETE", path, null)
    }

    suspend fun update(item: Item, list: ShoppingList, name: String, amount: Double, unitId: Long?) {
        send(
            "PUT",
            "/api/lists/${list.id}/items/${item.id}",
            """{"name":${name.asJson()},"amount":$amount,"unit_id":${unitId ?: "null"}}""",
        )
    }

    suspend fun attach(tag: Tag, item: Item, list: ShoppingList) {
        send("POST", "/api/lists/${list.id}/items/${item.id}/tags", """{"tag_id":${tag.id}}""")
    }

    suspend fun detach(tag: Tag, item: Item, list: ShoppingList) {
        send("DELETE", "/api/lists/${list.id}/items/${item.id}/tags/${tag.id}", null)
    }

    suspend fun delete(item: Item, list: ShoppingList) {
        send("DELETE", "/api/lists/${list.id}/items/${item.id}", null)
    }

    /** Empties the trolley in one request. N deletes can half-succeed, leaving a list
     * in a state nobody asked for. */
    suspend fun clearDone(list: ShoppingList) {
        send("DELETE", "/api/lists/${list.id}/items/done", null)
    }

    // ------------------------------------------------------------------- sync

    /**
     * Replays everything this device did while it could not reach the server.
     *
     * One request for the batch, and one answer per operation. Nothing here decides
     * what an answer means -- see [Outbox.drain], which is the only caller.
     */
    suspend fun sync(operations: List<SyncOperation>): List<AppliedOperation> {
        val body = json.encodeToString(Batch.serializer(), Batch(operations))
        val text = send("POST", "/api/sync", body)
        return json.decodeFromString(Replayed.serializer(), text).operations
    }

    // ------------------------------------------------------------------ lists

    suspend fun createList(name: String): ShoppingList =
        json.decodeFromString(send("POST", "/api/lists", """{"name":${name.asJson()}}"""))

    suspend fun rename(list: ShoppingList, name: String) {
        send("PUT", "/api/lists/${list.id}", """{"name":${name.asJson()}}""")
    }

    suspend fun delete(list: ShoppingList) {
        send("DELETE", "/api/lists/${list.id}", null)
    }

    // ---------------------------------------------------------------- sharing

    suspend fun whoAmI(): Me = get("/api/me")

    suspend fun people(list: ShoppingList): List<Person> = get("/api/lists/${list.id}/members")

    /**
     * A code to send, returned exactly once: only its hash is stored, so a lost code
     * is remade rather than found.
     *
     * The code alone, not a link. A link carries a host, and the host this app talks
     * to is 10.0.2.2 or somebody's laptop -- meaningless on the device it is being
     * sent to. Whoever receives it pastes it into an app that already knows which
     * server it is talking to.
     */
    suspend fun invite(list: ShoppingList): String {
        val body = send(
            "POST",
            "/api/lists/${list.id}/members/invites",
            """{"role":"editor"}""",
        )
        return json.decodeFromString<Invitation>(body).token
    }

    suspend fun revokeInvites(list: ShoppingList) {
        send("DELETE", "/api/lists/${list.id}/members/invites", null)
    }

    suspend fun join(token: String): ShoppingList =
        json.decodeFromString(send("POST", "/api/invites/$token", null))

    suspend fun remove(person: Person, list: ShoppingList) {
        send("DELETE", "/api/lists/${list.id}/members/${person.userId}", null)
    }

    // --------------------------------------------------------------- watching

    /**
     * Emits once each time this list changes, anywhere.
     *
     * The event carries a list id and nothing else. Carrying the rows would make this
     * app a second source of truth for order and content, and one dropped event would
     * leave it confidently disagreeing with the browser.
     */
    fun changes(list: ShoppingList): Flow<kotlin.Unit> =
        events("/api/lists/${list.id}/events")

    /**
     * Emits when the set of lists this person can see changes.
     *
     * A separate stream from a list's own, because it answers a different question: a
     * list that has just been made has no watchers, so announcing it to itself
     * reaches nobody.
     */
    fun listChanges(): Flow<kotlin.Unit> = events("/api/me/events")

    private fun events(path: String): Flow<kotlin.Unit> = callbackFlow {
        val request = Request.Builder()
            .url("$baseUrl$path")
            .header("Accept", "text/event-stream")
            .header("Authorization", "Bearer ${token() ?: ""}")
            .build()

        val source = EventSources.createFactory(client).newEventSource(
            request,
            object : EventSourceListener() {
                override fun onEvent(
                    eventSource: EventSource,
                    id: String?,
                    type: String?,
                    data: String,
                ) {
                    trySend(kotlin.Unit)
                }

                override fun onFailure(
                    eventSource: EventSource,
                    t: Throwable?,
                    response: Response?,
                ) {
                    // Closed rather than reported: losing the connection is ordinary
                    // — a lock screen, a doze, a server restart — and the collector
                    // reconnects.
                    close()
                }
            },
        )

        awaitClose { source.cancel() }
    }

    // --------------------------------------------------------------- plumbing

    private suspend inline fun <reified T> get(path: String): T =
        json.decodeFromString(send("GET", path, null))

    private suspend fun send(method: String, path: String, body: String?): String =
        withContext(Dispatchers.IO) {
            val bearer = token() ?: throw noToken()

            val request = Request.Builder()
                .url("$baseUrl$path")
                .header("Authorization", "Bearer $bearer")
                .apply {
                    if (body == null) {
                        method(method, if (method == "GET") null else EMPTY)
                    } else {
                        header("Content-Type", "application/json")
                        method(method, body.toRequestBody(JSON))
                    }
                }
                .build()

            val response = try {
                client.newCall(request).await()
            } catch (e: IOException) {
                throw ApiError.Transport(e)
            }

            response.use {
                val text = it.body?.string().orEmpty()
                when (it.code) {
                    in 200..299 -> text
                    401 -> throw ApiError.Unauthorized
                    403 -> throw refusalIn(text)
                    404 -> throw ApiError.NotFound
                    400, 409, 422 -> throw ApiError.BadInput(
                        messageIn(text) ?: "The server would not accept that."
                    )
                    else -> throw ApiError.Server(it.code)
                }
            }
        }

    /**
     * Which of the two 403s this is.
     *
     * The body carries `"reason": "not_admitted"` for the one that is about the
     * account rather than about a list. An older server sends no `reason` at all, and
     * the safe reading of silence is the narrower refusal.
     */
    private fun refusalIn(text: String): ApiError =
        if (Regex("\"reason\"\\s*:\\s*\"not_admitted\"").containsMatchIn(text)) {
            ApiError.NotAdmitted
        } else {
            ApiError.Forbidden
        }

    /** What a missing token means, which depends on whether anybody is signed in. */
    private fun noToken(): ApiError =
        if (remembered()) ApiError.Transport(IOException("No connection to sign in with")) else ApiError.Unauthorized

    /** The API answers errors as `{"error": "..."}`. */
    private fun messageIn(text: String): String? =
        Regex("\"error\"\\s*:\\s*\"([^\"]*)\"").find(text)?.groupValues?.get(1)

    private companion object {
        val JSON = "application/json".toMediaType()
        val EMPTY = "".toRequestBody(null)
    }
}

private suspend fun Call.await(): Response = suspendCancellableCoroutine { continuation ->
    enqueue(object : Callback {
        override fun onResponse(call: Call, response: Response) = continuation.resume(response)
        override fun onFailure(call: Call, e: IOException) = continuation.resumeWithException(e)
    })
    continuation.invokeOnCancellation { cancel() }
}

/** Quoted and escaped, so a name with a quote in it is a name rather than a syntax
 * error. */
private fun String.asJson(): String = buildString {
    append('"')
    this@asJson.forEach { c ->
        when (c) {
            '"' -> append("\\\"")
            '\\' -> append("\\\\")
            '\n' -> append("\\n")
            '\r' -> append("\\r")
            '\t' -> append("\\t")
            else -> if (c < ' ') append("\\u%04x".format(c.code)) else append(c)
        }
    }
    append('"')
}

private fun String.urlEncoded(): String =
    java.net.URLEncoder.encode(this, Charsets.UTF_8.name())
