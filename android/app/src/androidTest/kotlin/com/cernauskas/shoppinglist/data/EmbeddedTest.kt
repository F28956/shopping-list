package com.cernauskas.shoppinglist.data

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/**
 * The device's own server, on a device.
 *
 * Instrumented rather than a plain unit test, and it has to be: the library is compiled
 * for an Android ABI, so a JVM test on the build machine cannot load it. What is under
 * test here is *the crossing* — that the JNI names resolve, that a handle survives being
 * a `Long`, that strings arrive intact in both directions and that the envelope Kotlin
 * reads is the one Rust wrote. The behaviour inside is `domain`'s and is tested there,
 * twenty times over, in a language that can see it properly.
 *
 * Which is the point of this file being short. If it grew into a second suite of
 * shopping-list rules, that would be the mistake `web/embedded` exists to prevent.
 */
@RunWith(AndroidJUnit4::class)
class EmbeddedTest {

    private lateinit var database: File
    private var handle: Long = 0

    /** The `ok` payload, or a failure naming what came back instead. */
    private fun ok(answer: String?): kotlinx.serialization.json.JsonElement {
        assertNotNull("no answer at all -- did the library load?", answer)
        val envelope = Json.parseToJsonElement(answer!!).jsonObject
        val problem = envelope["error"]?.jsonPrimitive?.content
        assertTrue("the server refused it: $problem", problem == null)
        return envelope["ok"] ?: error("an envelope with neither ok nor error: $answer")
    }

    @Before
    fun open() {
        assertTrue("libembedded.so is not in this APK", Embedded.loaded)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        database = File(context.cacheDir, "embedded-test-${System.nanoTime()}.sqlite")
        handle = Embedded.open(database.path)
        assertTrue("the database would not open at ${database.path}", handle != 0L)
    }

    @After
    fun close() {
        if (handle != 0L) Embedded.close(handle)
        database.delete()
    }

    @Test
    fun aFreshDatabaseHasAPersonAndNoLists() {
        assertTrue("no person for this device", Embedded.me(handle) != 0L)
        assertEquals(0, ok(Embedded.lists(handle)).jsonArray.size)
    }

    @Test
    fun aListIsMadeAndReadBack() {
        val made = ok(Embedded.makeList(handle, "Household")).jsonObject
        assertEquals("Household", made["name"]?.jsonPrimitive?.content)

        val lists = ok(Embedded.lists(handle)).jsonArray
        assertEquals(1, lists.size)
        // Owner rather than viewer: a device that does not own its own list would hide
        // renaming and deleting on every one of them.
        assertEquals("owner", lists[0].jsonObject["role"]?.jsonPrimitive?.content)
        assertTrue(
            "no uuid to name it by",
            lists[0].jsonObject["uuid"]?.jsonPrimitive?.content?.isNotEmpty() == true,
        )
    }

    /**
     * The line goes through `parsing::add::resolve` on the other side, so this is also
     * the proof that the shared rules are reached from here — the amount and the unit
     * are read out of the words, not stored as typed.
     */
    @Test
    fun aTypedLineIsReadTheWayAPersonMeantIt() {
        val list = ok(Embedded.makeList(handle, "Shop")).jsonObject
        val id = list["id"]!!.jsonPrimitive.content.toLong()

        ok(Embedded.add(handle, id, "2 kg apples", null))

        val rows = ok(Embedded.items(handle, id)).jsonArray
        assertEquals(1, rows.size)
        val row = rows[0].jsonObject
        // Capitalised, as the server does it -- `web/parsing/src/add.rs`.
        assertEquals("Apples", row["name"]?.jsonPrimitive?.content)
        assertEquals(2.0, row["amount"]!!.jsonPrimitive.content.toDouble(), 0.0001)
    }

    @Test
    fun crossingSomethingOffSticks() {
        val list = ok(Embedded.makeList(handle, "Shop")).jsonObject
        val listId = list["id"]!!.jsonPrimitive.content.toLong()
        ok(Embedded.add(handle, listId, "milk", null))

        val itemId = ok(Embedded.items(handle, listId)).jsonArray[0]
            .jsonObject["id"]!!.jsonPrimitive.content.toLong()
        ok(Embedded.setDone(handle, itemId, true, 0))

        val row = ok(Embedded.items(handle, listId)).jsonArray[0].jsonObject
        assertTrue("it did not stay crossed off", row["done_at"]?.jsonPrimitive?.content != null)
    }

    /**
     * A refusal has to arrive as a refusal rather than as a crash or an empty answer.
     * The envelope is the contract; this is the half of it that is easy to leave
     * untested until something depends on it.
     */
    @Test
    fun aRefusalComesBackAsOne() {
        val answer = Embedded.items(handle, 9_999)
        assertNotNull(answer)
        val envelope = Json.parseToJsonElement(answer!!).jsonObject
        assertTrue(
            "a list that does not exist answered with $answer",
            envelope["error"] != null,
        )
    }

    /** The vocabulary is seeded by the same migration the server runs. */
    @Test
    fun theShippedUnitsAreHere() {
        val units = ok(Embedded.units(handle)).jsonArray
        assertTrue("no units at all", units.isNotEmpty())
        val names = units.map { it.jsonObject["name"]!!.jsonPrimitive.content }
        assertTrue("no kg among $names", names.contains("kg"))
    }

    /** A handle nobody opened must answer, not crash. */
    @Test
    fun noDatabaseIsAnAnswerRatherThanACrash() {
        val answer = Embedded.lists(0)
        assertNotNull(answer)
        assertTrue(
            "a zero handle answered with $answer",
            Json.parseToJsonElement(answer!!).jsonObject["error"] != null,
        )
    }
}
