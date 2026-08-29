package com.cernauskas.shoppinglist.data

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The two journeys a device can make between answering for itself and using a server.
 *
 * Instrumented because both ends are real: the device's own server is an Android
 * library, and the cache is Room. What is under test is that nothing is lost in either
 * direction — which is the failure this whole change is about, and which on the Apple
 * side was found by somebody's lists appearing to vanish.
 */
@RunWith(AndroidJUnit4::class)
class HandoverTest {

    private lateinit var backend: LocalBackend
    private lateinit var database: java.io.File
    private lateinit var cache: Cache

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun open() {
        assertTrue("libembedded.so is not in this APK", Embedded.loaded)
        database = java.io.File(context.cacheDir, "handover-${System.nanoTime()}.sqlite")
        // A database of this test's own. Opening the device's real one meant every case
        // inherited what the last had made -- six lists where one was expected.
        backend = LocalBackend.openAt(database) ?: error("no device backend")
        cache = Cache.inMemory(context)
    }

    @After
    fun close() {
        backend.close()
        database.delete()
    }

    /**
     * The whole journey: made on a device with no server, and afterwards in the cache as
     * a list no server has heard of — which is exactly what the queue is built from.
     */
    @Test
    fun whatIsOnTheDeviceReachesTheCache() = runBlocking {
        val list = backend.createList("Household")
        backend.add("2 kg apples", list)
        backend.add("milk", list)

        assertTrue("the handover refused", backend.handOverToAServer(cache))

        val carried = cache.lists()
        assertEquals(1, carried.size)
        assertEquals("Household", carried[0].name)
        // Local, which is what "no server has heard of this" is written as -- and what
        // the handover walks for.
        assertTrue("it arrived as something a server already knows", carried[0].id < 0)
        assertEquals(2, cache.items(carried[0]).size)
    }

    /**
     * The uuids have to survive, or the first drain makes a second copy of everything.
     *
     * `make_list` and `add` are both idempotent by uuid on the server. Minting new ones
     * here -- which is what `makeListHere` does, and what the obvious implementation
     * would reuse -- would mean the device and the server disagreed about the name of
     * every row from the moment they met.
     */
    @Test
    fun theNamesTheServerWillBeToldAreTheDevicesOwn() = runBlocking {
        val list = backend.createList("Household")
        backend.add("apples", list)
        val apples = backend.items(list).items.first()

        backend.handOverToAServer(cache)

        val carried = cache.lists().first()
        assertEquals("the list was renamed on the way", list.uuid, carried.uuid)
        assertEquals("the row was renamed on the way", apples.uuid, cache.items(carried)[0].uuid)
    }

    /**
     * Two lists, because the ids are minted here and the obvious loop gives the second
     * list's rows the ids the first list's already have -- which is a primary key.
     */
    @Test
    fun aSecondListDoesNotCollideWithTheFirst() = runBlocking {
        val home = backend.createList("Home")
        val boat = backend.createList("Boat")
        backend.add("apples", home)
        backend.add("rope", boat)

        backend.handOverToAServer(cache)

        assertEquals("one of the two lists was lost", 2, cache.lists().size)
        for (list in cache.lists()) {
            assertEquals("${list.name} came out wrong", 1, cache.items(list).size)
        }
    }

    /** Running it twice must not make a second copy. */
    @Test
    fun handingOverTwiceDoesNotDuplicate() = runBlocking {
        val list = backend.createList("Household")
        backend.add("apples", list)

        backend.handOverToAServer(cache)
        backend.handOverToAServer(cache)

        assertEquals("the same list arrived twice", 1, cache.lists().size)
        assertEquals(1, cache.items(cache.lists().first()).size)
    }

    /**
     * And nothing is taken away. This is what makes adopting a server reversible before
     * anybody has proved the server works.
     */
    @Test
    fun theDeviceKeepsEverythingItHandedOver() = runBlocking {
        val list = backend.createList("Household")
        backend.add("apples", list)

        backend.handOverToAServer(cache)

        assertEquals("the device lost its list", 1, backend.lists().items.size)
        assertEquals("the device lost its shopping", 1, backend.items(list).items.size)
    }

    /**
     * Nothing is queued on a device kept to itself.
     *
     * Standalone is not server mode with the server unreachable; it is a device where
     * its own database is the truth and there is nobody to tell. A queue there is a log
     * of everything that has ever happened, written for a reader that does not exist.
     */
    @Test
    fun aDeviceWithNoServerQueuesNothing() = runBlocking {
        val quiet = Cache.inMemory(context, sending = { false })
        val list = quiet.makeListHere("Household", ownedBy = 0)
        quiet.outbox.makeList(list)

        assertEquals("a device with nobody to tell wrote it down anyway", 0, quiet.outbox.waiting())
    }

    /** And a device that has one queues everything, which is the other half of the rule. */
    @Test
    fun aDeviceWithAServerQueuesEverything() = runBlocking {
        val loud = Cache.inMemory(context, sending = { true })
        val list = loud.makeListHere("Household", ownedBy = 0)
        loud.outbox.makeList(list)

        assertEquals(1, loud.outbox.waiting())
    }
}
