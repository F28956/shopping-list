package com.cernauskas.shoppinglist.data

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * A server, with the memory and the queue a server needs.
 *
 * The Kotlin twin of `ShoppingListTests/CachingBackendTests.swift`, and the same cases:
 * these behaviours used to live in two view models, where they were reachable only
 * through a screen. Testing them where they now live is the point of the move.
 *
 * Every call here goes to an address nothing answers, so each takes the unreachable
 * path. That is not a corner case; it is a phone in a shop.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class CachingBackendTest {

    // Robolectric's own, rather than `androidx.test.core`: this module does not depend
    // on the test-core artifact and one class is not worth adding it for.
    private val context get() = org.robolectric.RuntimeEnvironment.getApplication()

    /** Against a port nothing listens on, so every request fails as transport. */
    private fun backend(cache: Cache): CachingBackend =
        CachingBackend(
            Api(server = { "http://127.0.0.1:1" }, token = { "none" }, remembered = { true }),
            cache,
        )

    private fun cache() = Cache.inMemory(context, sending = { true })

    private fun list(id: Long = 1) =
        ShoppingList(id = id, uuid = "list-$id", name = "Shop", ownerId = 9, role = Role.EDITOR)

    /**
     * The bug the cache exists for: an app that says "you have no lists" when what it
     * means is that it could not find out.
     */
    @Test
    fun `an unreachable server answers from what was last seen`() = runBlocking {
        val cache = cache()
        cache.rememberLists(listOf(list()))

        val answer = backend(cache).lists()

        assertEquals(listOf("Shop"), answer.items.map { it.name })
    }

    /** And it must say so, or the screen cannot tell "nothing" from "I don't know". */
    @Test
    fun `being unable to reach the server is not an empty list`() = runBlocking {
        val backend = backend(cache())

        val answer = backend.lists()

        assertTrue(answer.items.isEmpty())
        assertFalse("a screen would have shown `no lists`", backend.reachable)
    }

    /** A list made with no signal is a list. Where it goes meanwhile is this type's job. */
    @Test
    fun `a list made with no signal is written down and queued`() = runBlocking {
        val cache = cache()
        val backend = backend(cache)

        val made = backend.createList("Household")

        assertEquals("Household", made.name)
        assertEquals(listOf("Household"), cache.lists().map { it.name })
        assertEquals("nothing was queued for the server", 1, backend.pending)
        assertFalse(backend.reachable)
    }

    /**
     * The rule that stops a successful read visibly undoing a tick that is still queued.
     *
     * The server has not been told, so it answers with the old state, and the row would
     * flick back for as long as the queue is stuck.
     */
    @Test
    fun `a queued tick is laid back over what was read`() = runBlocking {
        val cache = cache()
        val backend = backend(cache)
        val list = list()
        cache.rememberLists(listOf(list))
        val milk = Item(id = 1, uuid = "milk", name = "Milk", amount = 1.0)
        cache.rememberItems(list, listOf(milk))

        backend.setDone(milk, list, done = true)

        assertTrue(
            "the queued tick was undone by a read",
            backend.items(list).items.first { it.uuid == "milk" }.isDone,
        )
    }

    /**
     * **Only** rows this device made are carried across a read.
     *
     * Any queued operation used to qualify, which meant a tick queued against a row
     * somebody else had deleted put it back on screen: present here, gone everywhere
     * else, impossible to be rid of.
     */
    @Test
    fun `a row somebody else deleted does not come back as a ghost`() = runBlocking {
        val cache = cache()
        val backend = backend(cache)
        val list = list()
        cache.rememberLists(listOf(list))
        val theirs = Item(id = 3, uuid = "theirs", name = "Bread", amount = 1.0)
        cache.rememberItems(list, listOf(theirs))

        // Ticked here, deleted there: the queue holds a tick against a row that is gone,
        // and the cache no longer has it either.
        backend.setDone(theirs, list, done = true)
        cache.rememberItems(list, emptyList())

        assertTrue("a row somebody else deleted came back", backend.items(list).items.isEmpty())
    }

    /** Nothing queued is a real answer, and the honest one. */
    @Test
    fun `a backend with nothing queued says so`() = runBlocking {
        assertEquals(0, backend(cache()).pending)
    }

    /**
     * A device kept to itself queues nothing, even on this path.
     *
     * `CachingBackend` is reached in standalone only when the device's own database will
     * not open. It must not start a queue there: there is nobody to drain it to, and it
     * would grow without bound behind a dot that is hidden.
     */
    @Test
    fun `with nobody to tell, nothing is queued`() = runBlocking {
        val quiet = Cache.inMemory(context, sending = { false })
        val backend = backend(quiet)

        backend.createList("Household")

        assertEquals("a device with nobody to tell started a queue", 0, backend.pending)
    }
}
