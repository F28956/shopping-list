package com.cernauskas.shoppinglist.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * What somebody types, and what is stored.
 *
 * The same cases as `ios/ShoppingListTests/ServerAddressTests.swift`, on purpose: two
 * clients that disagree about what an address means are two clients that talk to
 * different servers from the same typing.
 *
 * Robolectric because `Uri` is Android's, and a hand-rolled parser here would be a
 * second set of rules to keep in step with the first.
 */
@RunWith(RobolectricTestRunner::class)
// Pinned because Robolectric ships support for one SDK at a time and the app targets a
// newer one. `Uri.parse` has not changed in years, so an older emulated SDK tests the
// same parser; this line moves when Robolectric catches up.
@Config(sdk = [35])
class ServerAddressTest {
    private fun origin(typed: String): String? =
        ServerAddress.parse(typed).getOrNull()?.origin

    private fun problem(typed: String): ServerAddress.Problem? =
        ServerAddress.parse(typed).exceptionOrNull()?.addressProblem

    @Test
    fun `an ordinary address survives unchanged`() {
        assertEquals("https://shopping.example.com", origin("https://shopping.example.com"))
    }

    @Test
    fun `a missing scheme becomes https`() {
        assertEquals("https://shopping.example.com", origin("shopping.example.com"))
        assertEquals("https://shopping.example.com:8080", origin("shopping.example.com:8080"))
    }

    @Test
    fun `a trailing slash goes`() {
        assertEquals("https://shopping.example.com", origin("https://shopping.example.com/"))
    }

    @Test
    fun `the host is lowercased and space ignored`() {
        assertEquals("https://shopping.example.com", origin("  HTTPS://Shopping.Example.COM  "))
    }

    /**
     * The trap. A base with a path silently loses it when a relative path is appended,
     * so it is refused rather than repaired.
     */
    @Test
    fun `a path is refused rather than dropped`() {
        assertEquals(ServerAddress.Problem.NOT_JUST_AN_ORIGIN, problem("https://example.com/lists"))
        assertEquals(ServerAddress.Problem.NOT_JUST_AN_ORIGIN, problem("https://example.com/api/"))
        assertEquals(ServerAddress.Problem.NOT_JUST_AN_ORIGIN, problem("https://example.com?x=1"))
        assertEquals(ServerAddress.Problem.NOT_JUST_AN_ORIGIN, problem("https://example.com#top"))
    }

    @Test
    fun `ports are kept only when they say something`() {
        assertEquals("https://example.com:8443", origin("https://example.com:8443"))
        assertEquals("https://example.com", origin("https://example.com:443"))
        assertEquals("http://example.com", origin("http://example.com:80"))
        assertEquals("http://10.0.2.2:8080", origin("http://10.0.2.2:8080"))
    }

    /**
     * C6. Which way this goes is decided by the build, not by the caller — there is no
     * longer a parameter to pass, so no call site can opt itself out. Both halves are
     * said here, so that whichever build this runs under it asserts the rule that build
     * is meant to keep.
     */
    @Test
    fun `cleartext follows the build`() {
        if (ServerAddress.allowsCleartext()) {
            assertEquals("http://example.com", origin("http://example.com"))
        } else {
            assertEquals(ServerAddress.Problem.INSECURE, problem("http://example.com"))
        }
        assertEquals("https://example.com", origin("https://example.com"))
    }

    @Test
    fun `nonsense is refused`() {
        assertEquals(ServerAddress.Problem.EMPTY, problem(""))
        assertEquals(ServerAddress.Problem.EMPTY, problem("   "))
        assertEquals(ServerAddress.Problem.NOT_AN_ADDRESS, problem("ftp://example.com"))
        assertEquals(ServerAddress.Problem.NOT_AN_ADDRESS, problem("https://"))
        assertNull(origin("https://"))
    }

    @Test
    fun `every problem says something`() {
        for (problem in ServerAddress.Problem.entries) {
            assert(problem.sentence().isNotEmpty())
        }
    }
}
