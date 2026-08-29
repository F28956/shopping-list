package com.cernauskas.shoppinglist.diagnostics

import com.cernauskas.shoppinglist.data.ServerAddress
import com.cernauskas.shoppinglist.data.ServerDirectory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * What a device may say about itself, and where the line is.
 *
 * Two rules, and both of them are promises the settings screen makes out loud: a device
 * answering for itself collects and reports nothing, and nothing that came from a person
 * can become a label.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class MetricsTest {

    private val context get() = org.robolectric.RuntimeEnvironment.getApplication()

    @Before
    fun start() {
        ServerDirectory.start(context)
        DiagnosticsSettings.start(context)
        Metrics.forget()
    }

    /**
     * The promise `Capabilities.syncing` already makes about the screens, kept about
     * the numbers too.
     *
     * There is no far end, so there is no latency, no queue and no stream — every
     * measurement here would be a measurement of a relationship that does not exist,
     * and collecting one and merely declining to send it is not the same promise.
     */
    @Test
    fun `a device answering for itself collects nothing`() {
        ServerDirectory.onlyThisDevice()

        Metrics.launched()
        Metrics.request(Route.LISTS, Outcome.OK, 40)
        Metrics.queueDepth(7)
        Metrics.drained(sent = 3, waiting = 0, lost = 1, refused = false)
        Metrics.stream(opened = true)
        Metrics.reachability(reachable = false)

        assertEquals("a standalone device was counting things", 0, Metrics.recorded())
    }

    @Test
    fun `a device with a server counts them`() {
        ServerDirectory.remember(ServerAddress.parse("https://shopping.example").getOrThrow())

        Metrics.launched()
        Metrics.request(Route.LISTS, Outcome.OK, 40)
        Metrics.queueDepth(7)

        assertTrue("nothing was counted with a server configured", Metrics.recorded() >= 3)
    }

    /**
     * A path is not a label, and this is why.
     *
     * `/api/admissions/{email}` has somebody's address in it, and a metric labelled with
     * the path would carry it to whatever collector is configured. Anything the closed
     * set does not recognise becomes `other`, which is the safe direction: a new route
     * shows up as nothing rather than as itself.
     */
    @Test
    fun `an address in a path never becomes a label`() {
        val route = Route.of("/api/admissions/somebody%40example.com/owner")

        assertEquals(Route.ADMISSIONS, route)
        assertEquals("admissions", route.name.lowercase())
    }

    @Test
    fun `the routes worth telling apart are told apart`() {
        assertEquals(Route.LISTS, Route.of("/api/lists?order_by=updated_at&size=500"))
        assertEquals(Route.LIST, Route.of("/api/lists/12"))
        assertEquals(Route.ITEMS, Route.of("/api/lists/12/items?size=500"))
        assertEquals(Route.ITEM, Route.of("/api/lists/12/items/44"))
        assertEquals(Route.ITEM_DONE, Route.of("/api/lists/12/items/44/done"))
        assertEquals(Route.ITEM_DONE, Route.of("/api/lists/12/items/done"))
        assertEquals(Route.SYNC, Route.of("/api/sync"))
        assertEquals(Route.EVENTS, Route.of("/api/me/events"))
        assertEquals(Route.EVENTS, Route.of("/api/lists/12/events"))
        assertEquals(Route.INVITES, Route.of("/api/lists/12/members/invites"))
        assertEquals("an unknown route is not passed through", Route.OTHER, Route.of("/api/wat/9"))
    }
}
