package com.cernauskas.shoppinglist.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Reading the token, and the server, out of whatever somebody pastes.
 *
 * Robolectric because reading the *server* out of a link goes through `Uri`, which is
 * Android's -- the same reason `ServerAddressTest` needs it. Reading the token alone
 * does not, and used to be all this file did.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class JoinLinkTest {
    /**
     * The shape a server issues: the token after the `#`, where no proxy and no access
     * log between here and somebody's home server ever sees it. A fragment is the one
     * part of a URL a browser does not send.
     */
    @Test
    fun `a token is found in the fragment`() {
        assertEquals("abc123", tokenIn("http://localhost:8080/join#abc123"))
        assertEquals("abc123", tokenIn("https://shopping.example/join#abc123"))
        // pasted with the whitespace a chat app leaves behind
        assertEquals("abc123", tokenIn("  http://localhost:8080/join#abc123 \n"))
    }

    /**
     * The older shape, with the token in the path. Still read, so a link sent before a
     * server was updated keeps working in somebody's inbox.
     */
    @Test
    fun `a token is still found in the path`() {
        assertEquals("abc123", tokenIn("http://localhost:8080/join/abc123"))
        assertEquals("abc123", tokenIn("https://shopping.example/join/abc123"))
    }

    /** Just the token, which is what somebody who read the link sends on. */
    @Test
    fun `a bare token is taken as it is`() {
        assertEquals("abc123", tokenIn("abc123"))
        assertEquals("abc123", tokenIn("  abc123  "))
    }

    @Test
    fun `these are not links`() {
        assertNull(tokenIn(""))
        assertNull(tokenIn("   "))
        // a sentence, not a link: somebody pasted the whole message
        assertNull(tokenIn("here is the link I promised"))
        // a link to nothing in particular
        assertNull(tokenIn("http://localhost:8080/"))
        // the join page with no invitation on it
        assertNull(tokenIn("http://localhost:8080/join"))
        assertNull(tokenIn("http://localhost:8080/join#"))
    }

    /**
     * A path-shaped link that happens to end in a bare `#`.
     *
     * The fragment is empty, which is not the same as there being no token — and
     * reading it as "no token" is why this worked on an iPhone and not here. Mail
     * clients and chat apps add stray characters to the end of a URL often enough that
     * this is not a hypothetical.
     */
    @Test
    fun `an empty fragment does not hide a token in the path`() {
        assertEquals(
            "abc123",
            tokenIn("http://localhost:8080/join/abc123#"),
        )
    }

    /**
     * A link from a server mounted under a path names that path too.
     *
     * Keeping only the host would offer an address serving somebody else's
     * application, and the person pasting has no way to notice the prefix was dropped.
     */
    @Test
    fun `a link names the path it is mounted under`() {
        for ((pasted, expected) in listOf(
            "https://example.com/sl/join#abc123" to "https://example.com/sl",
            "https://example.com/sl/join/abc123" to "https://example.com/sl",
            "https://example.com/apps/shopping/join#abc" to "https://example.com/apps/shopping",
            // No prefix, which is every link issued before this existed.
            "https://example.com/join#abc123" to "https://example.com",
        )) {
            assertEquals(pasted, expected, serverAddressIn(pasted)?.written)
        }
    }
}
