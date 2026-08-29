package com.cernauskas.shoppinglist.diagnostics

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/**
 * The two promises the log makes, held here rather than in a review comment.
 *
 * The first is that nothing goes anywhere until somebody says so, and the second is
 * that `info`, `warn` and `error` cannot carry personal data. The second is the one
 * worth the most: it is a promise about a file people attach to bug reports, and a
 * shopping list says more about somebody than it looks like it does — see
 * docs/self-hosting.md, S8.
 *
 * Every case writes to a directory of this test's own. Writing to the real one meant
 * asserting against whatever else on the machine had logged, which is the same lesson
 * `LocalBackend.openAt` learned.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class DiagnosticsTest {

    private lateinit var directory: File

    private val context get() = org.robolectric.RuntimeEnvironment.getApplication()

    @Before
    fun start() {
        directory = File.createTempFile("diagnostics", "").let {
            it.delete()
            File(it.path + "-dir")
        }
        DiagnosticsSettings.start(context)
        Diagnostics.startAt(directory)
        Diagnostics.forget()
    }

    /** Everything written so far, as one string. */
    private fun written(): String {
        Diagnostics.settle()
        return directory.listFiles().orEmpty()
            .filter { it.name.endsWith(".log") }
            .joinToString("\n") { it.readText() }
    }

    // MARK: - Off until somebody turns it on

    /**
     * The default, and the reason it is not silence: the two levels above it carry
     * nothing personal by construction, so there is nothing to opt into — and a crash
     * nobody recorded is a crash nobody can fix.
     */
    @Test
    fun `a fresh install records problems and nothing else`() {
        assertEquals(Level.WARN, Level.default)
        assertFalse("info would go somewhere on a fresh install", Level.default.admits(Level.INFO))
        assertFalse(Level.default.admits(Level.DEBUG))
        assertTrue(Level.default.admits(Level.ERROR))
    }

    @Test
    fun `nothing below the chosen level reaches the file`() {
        Diagnostics.setLevel(Level.WARN)
        Diagnostics.forget()

        Diagnostics.info(Event.BACKEND_READ, Fact.of(Field.COUNT, 3))
        Diagnostics.warn(Event.CACHE_WRITE_FAILED)

        val text = written()
        assertFalse("an info line was written at warn", text.contains("backend.read"))
        assertTrue(text.contains("cache.write_failed"))
    }

    @Test
    fun `turning the level down lets the quieter lines through`() {
        Diagnostics.setLevel(Level.INFO)
        Diagnostics.forget()

        Diagnostics.info(Event.BACKEND_READ, Fact.of(Field.COUNT, 3))

        assertTrue(written().contains("backend.read count=3"))
    }

    /** The switch is storage, so it has to survive the process that set it. */
    @Test
    fun `the chosen level is remembered`() {
        Diagnostics.setLevel(Level.TRACE)

        assertEquals(Level.TRACE, DiagnosticsSettings.level)
        assertEquals(Level.TRACE, Level.named("trace"))
        assertEquals("an unknown level is not silently obeyed", Level.WARN, Level.named("shout"))
    }

    // MARK: - The redaction rule

    /**
     * The important one.
     *
     * There is no way to hand `info` a name, so the way this is proved is by trying
     * every route a name could take at that level and reading the file back. The
     * length goes in and the characters do not.
     */
    @Test
    fun `info cannot carry an item name`() {
        Diagnostics.setLevel(Level.INFO)
        Diagnostics.forget()

        val name = "pregnancy test"

        Diagnostics.info(
            Event.BACKEND_WRITE,
            Fact.of(Field.LIST, 12L),
            Fact.of(Field.ITEM, 44L),
            Fact.length(Field.LENGTH, name),
            Fact.of(Field.OUTCOME, Outcome.OK),
            Fact.failure(IllegalStateException(name)),
        )

        val text = written()
        assertFalse("an item name reached an info line", text.contains(name))
        assertFalse("an item name reached an info line", text.contains("pregnancy"))
        assertTrue("the length was dropped too", text.contains("length=${name.length}"))
        assertTrue("the failure lost its class as well as its message",
            text.contains("reason=IllegalStateException"))
    }

    /**
     * The same at the two levels above it, because a warning about a failed write is
     * exactly where somebody would reach for the row that failed.
     */
    @Test
    fun `warn and error cannot carry one either`() {
        Diagnostics.setLevel(Level.WARN)
        Diagnostics.forget()

        val name = "co-codamol"
        Diagnostics.warn(Event.CACHE_WRITE_FAILED, Fact.length(Field.LENGTH, name))
        Diagnostics.error(Event.NATIVE_FAILED, Fact.failure(RuntimeException(name)))

        val text = written()
        assertFalse(text.contains(name))
    }

    /**
     * And the structural half, which is what keeps the case above true tomorrow.
     *
     * `info`, `warn` and `error` take a [Fact], and a `Fact` can only be built through
     * the factories on its companion. If one of them ever takes a string as its *value*
     * then an item name has a way in, and adding it should fail the build rather than
     * the review. `length` is the exception and takes one deliberately — it keeps the
     * count and drops the characters, which the case above checks.
     */
    @Test
    fun `no fact can be built out of a string`() {
        val offenders = Fact.Companion::class.java.declaredMethods
            .filter { java.lang.reflect.Modifier.isPublic(it.modifiers) }
            .filterNot { it.isSynthetic }
            .filter { it.name != "length" }
            .filter { method -> method.parameterTypes.any { it == String::class.java } }
            .map { it.name }

        assertEquals(
            "a Fact factory now takes a string, which is a way for a list's contents " +
                "to reach an info line",
            emptyList<String>(),
            offenders,
        )
    }

    // MARK: - The levels that may say anything

    @Test
    fun `debug is not even evaluated when it is off`() {
        Diagnostics.setLevel(Level.INFO)
        Diagnostics.forget()

        var asked = false
        Diagnostics.debug(Event.BACKEND_READ) { asked = true; "milk, bread" }

        assertFalse("the contents were assembled for a level nobody turned on", asked)
        assertFalse(written().contains("milk, bread"))
    }

    /** The other half of the deal: turned on, it says everything, which is what the
     * settings screen warns about before it lets anybody choose it. */
    @Test
    fun `debug says what is on the list once somebody asks for it`() {
        Diagnostics.setLevel(Level.DEBUG)
        Diagnostics.forget()

        Diagnostics.debug(Event.BACKEND_READ, Fact.of(Field.LIST, 3L)) { "milk, bread" }

        val text = written()
        assertTrue(text.contains("backend.read list=3"))
        assertTrue(text.contains("| milk, bread"))
    }

    @Test
    fun `both levels that reveal contents say so`() {
        assertTrue(Level.TRACE.revealsContents)
        assertTrue(Level.DEBUG.revealsContents)
        assertFalse(Level.INFO.revealsContents)
        assertFalse(Level.WARN.revealsContents)
        assertFalse(Level.ERROR.revealsContents)
    }

    // MARK: - The file

    /** A log that can grow without limit is a bug report nobody can send and a phone
     * that fills up. */
    @Test
    fun `the file stops growing`() {
        Diagnostics.setLevel(Level.INFO)
        Diagnostics.forget()

        repeat(20_000) { Diagnostics.info(Event.REQUEST, Fact.of(Field.MILLIS, it)) }
        Diagnostics.settle()

        assertTrue("the log grew past its cap", Diagnostics.sizeBytes() <= 512L * 1024)
        assertTrue("the log kept nothing", Diagnostics.sizeBytes() > 0)
    }

    @Test
    fun `an export is one archive of everything kept`() {
        Diagnostics.setLevel(Level.INFO)
        Diagnostics.forget()
        Diagnostics.info(Event.APP_LAUNCHED)

        val archive = Diagnostics.packUp()

        assertTrue(archive != null && archive.exists() && archive.length() > 0)
    }

    @Test
    fun `there is nothing to export when nothing has been written`() {
        Diagnostics.setLevel(Level.ERROR)
        Diagnostics.forget()

        assertEquals(null, Diagnostics.packUp())
    }
}
