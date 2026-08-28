package com.cernauskas.shoppinglist.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Reading the token out of whatever somebody pastes. */
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
}
