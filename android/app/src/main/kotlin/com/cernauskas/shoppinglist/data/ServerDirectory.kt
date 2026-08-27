package com.cernauskas.shoppinglist.data

import android.content.Context
import android.content.SharedPreferences
import com.cernauskas.shoppinglist.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject
import java.io.IOException
import javax.net.ssl.SSLException

/**
 * Which server this device talks to, and how it came to believe that.
 *
 * A self-hosted app cannot be built pointed anywhere, because the answer is different
 * for everybody. So the address is stored, and `BuildConfig.API_BASE_URL` becomes what
 * a fresh install starts from rather than what it is stuck with.
 *
 * The iOS half is `ios/Shared/Sources/ServerDirectory.swift` and the two behave the
 * same on purpose, down to the three states of the stored value.
 */
object ServerDirectory {
    private const val PREFS = "server"
    private const val KEY = "address"

    private lateinit var prefs: SharedPreferences

    /** Called once from `Application.onCreate`, before anything asks. */
    fun start(context: Context) {
        prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    }

    /**
     * One key, three states, and the third is the one that is easy to miss.
     *
     * * **Absent** — nobody has ever been asked, so the build setting applies.
     * * **A value** — what somebody entered.
     * * **Empty** — somebody deliberately cleared it, and the build setting must *not*
     *   apply. Without this state, "change server" on a build compiled with an address
     *   would clear the stored one and fall straight back to the built-in, which is a
     *   button that appears to work and does nothing.
     */
    val current: ServerAddress?
        get() {
            val stored = prefs.getString(KEY, null) ?: return built
            if (stored == ON_DEVICE_ONLY) return null
            return ServerAddress.parse(stored, allowingCleartext = true).getOrNull()
        }

    /**
     * The reserved value for "this device on its own". A fourth state on the same key
     * rather than a second key, because it is the same question with another answer.
     */
    private const val ON_DEVICE_ONLY = "local"

    /**
     * Records that this device is on its own.
     *
     * The app then works exactly as it does with no signal, which is not a
     * coincidence: everything queues to the outbox and shows from the cache, and
     * attaching a server later drains the queue into it. "No server" and "no signal"
     * are the same state, and the app already knew how to be in one of them.
     */
    fun onlyThisDevice() {
        prefs.edit().putString(KEY, ON_DEVICE_ONLY).apply()
    }

    /**
     * Whether this device has no server *and has said so*, as opposed to not having
     * been asked.
     */
    val isOnDeviceOnly: Boolean
        get() = prefs.getString(KEY, null) == ON_DEVICE_ONLY

    /**
     * What the build was pointed at.
     *
     * Cleartext is allowed here whatever the build says: this came from somebody
     * compiling the app rather than from a text field, and refusing it would only break
     * an emulator talking to a server on the same desk.
     */
    private val built: ServerAddress?
        get() = ServerAddress.parse(BuildConfig.API_BASE_URL, allowingCleartext = true).getOrNull()

    /**
     * Whether anybody has to be asked. False on a debug build, which has one, and
     * false once somebody has said "this device only".
     */
    val needsAnAddress: Boolean
        get() = current == null && !isOnDeviceOnly

    /**
     * Records an address that has been checked, and says whether it is a *different*
     * server from the one before — which is the caller's cue to throw everything local
     * away, for the reason [forget] gives.
     */
    fun remember(address: ServerAddress): Boolean {
        val changed = current != address
        prefs.edit().putString(KEY, address.origin).apply()
        return changed
    }

    /**
     * Forgets the stored address, so the next launch asks again.
     *
     * **Callers must also clear the cache and sign out.** Not a precaution: the cache
     * holds rows keyed by ids and uuids the old server minted, and history and
     * suggestions belong to an account on it. Carrying them across would show one
     * server's lists under another server's name.
     */
    fun forget() {
        // Emptied rather than removed: removing it would mean "never asked", and a build
        // with a compiled-in address would answer with that one — see [current].
        prefs.edit().putString(KEY, "").apply()
    }

    /** What a server says about itself. The other end is `GET /api/server`. */
    data class About(
        val name: String,
        val version: String,
        /** `open`, `closed` or `unclaimed`. */
        val admission: String,
    ) {
        /**
         * Nobody owns this server yet, so the first person to arrive claims it with the
         * code from its log rather than signing in.
         */
        val isUnclaimed: Boolean get() = admission == "unclaimed"

        /**
         * Whether a stranger will be let in, so a sign-in screen can stop promising a
         * refusal that will not come.
         */
        val admitsAnyone: Boolean get() = admission == "open"
    }

    /** Why an address was not accepted, in the words the screen says. */
    enum class Refusal {
        UNREACHABLE,
        NOT_THIS_SOFTWARE,
        CERTIFICATE_REFUSED,
        ;

        fun sentence(): String = when (this) {
            UNREACHABLE ->
                "Cannot reach that address. Check it, and check you are on the same " +
                    "network as the server."
            NOT_THIS_SOFTWARE -> "Something is running there, but it is not a Shopping List server."
            CERTIFICATE_REFUSED -> "That server's certificate could not be verified."
        }
    }

    /** The name the server answers with. A mismatch is refused. */
    const val SOFTWARE = "shopping-list"

    /**
     * Asks an address whether it is a Shopping List server.
     *
     * C2: a regular expression proves the string is a URL. It does not prove there is a
     * server there, that it is *this* server, or that TLS will negotiate — and all three
     * fail in ways a person can fix. `GET /healthz` would not do either, since every
     * health endpoint on the internet returns `ok`.
     */
    suspend fun ask(
        address: ServerAddress,
        client: OkHttpClient = OkHttpClient(),
    ): Result<About> = withContext(Dispatchers.IO) {
        val request = Request.Builder().url("${address.origin}/api/server").build()

        try {
            client.newCall(request).execute().use { response ->
                if (!response.isSuccessful) return@withContext Result.failure(Refused(Refusal.NOT_THIS_SOFTWARE))

                val body = response.body?.string().orEmpty()
                val about = runCatching {
                    val json = JSONObject(body)
                    About(
                        name = json.getString("name"),
                        version = json.optString("version"),
                        admission = json.optString("admission"),
                    )
                }.getOrNull()

                if (about == null || about.name != SOFTWARE) {
                    return@withContext Result.failure(Refused(Refusal.NOT_THIS_SOFTWARE))
                }

                Result.success(about)
            }
        } catch (e: SSLException) {
            // Told apart from an ordinary failure because it is fixed differently: a
            // certificate is repaired on the server, and a wrong address is retyped.
            Result.failure(Refused(Refusal.CERTIFICATE_REFUSED))
        } catch (e: IOException) {
            Result.failure(Refused(Refusal.UNREACHABLE))
        }
    }

    /** Carries a [Refusal] through `Result`, which wants a `Throwable`. */
    class Refused(val refusal: Refusal) : Exception(refusal.sentence())
}

/** The [ServerDirectory.Refusal] behind a failure, for a screen that has to say something. */
val Throwable.serverRefusal: ServerDirectory.Refusal?
    get() = (this as? ServerDirectory.Refused)?.refusal
