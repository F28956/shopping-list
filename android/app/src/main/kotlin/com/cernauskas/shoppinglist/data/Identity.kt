package com.cernauskas.shoppinglist.data

import android.content.Context
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.NoCredentialException
import com.cernauskas.shoppinglist.BuildConfig
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential

/**
 * Who is signed in, and the token to prove it.
 *
 * Credential Manager rather than the old Google Sign-In SDK, which is deprecated. The
 * token it returns is addressed to the **web** client id, because that is what is
 * handed over as `serverClientId` — the Android OAuth client, registered against the
 * package name and the signing certificate's SHA-1, is what lets Google attest the
 * app and is named nowhere in this code.
 */
class Identity(private val context: Context) {

    /**
     * That somebody is signed in on this device, and what to call them.
     *
     * Kept because asking Google needs a connection, and a phone opened in a shop with
     * no signal would otherwise show the sign-in screen with the cached list stranded
     * behind it -- which is the case this whole piece of work is about. What is
     * remembered is a boolean and a display name: no token, nothing that grants
     * anything, and nothing that outlives signing out.
     */
    private val remembered =
        context.getSharedPreferences("session", Context.MODE_PRIVATE)

    /** Whether somebody has signed in on this device and not signed out. */
    val isRemembered: Boolean get() = remembered.getBoolean(SIGNED_IN, false)

    /// Records that somebody is signed in, and what to call them.
    ///
    /// The flag is set whether or not there is a name: an account with no display name
    /// is still an account, and making the flag depend on the name meant one of those
    /// was never remembered at all.
    private fun remember(name: String?) {
        remembered.edit().putBoolean(SIGNED_IN, true).putString(NAME, name).apply()
    }

    private val rememberedName: String? get() = remembered.getString(NAME, null)

    /**
     * Why a session ended, because the two are not the same thing.
     *
     * Somebody tapping Sign out is leaving, and their shopping should not be waiting on
     * the phone for whoever picks it up next. A server refusing a token is the same
     * person with an expired credential, and throwing away their unsent changes over it
     * would be losing work to a clock.
     */
    sealed interface Departure {
        data object Deliberate : Departure
        data class Refused(val problem: String? = null) : Departure
    }

    sealed interface State {
        data object Unknown : State
        /** Signed out, and why — or null when nothing was attempted. A failed sign-in
         * used to land here with nothing to show, which on a button that is already
         * on the sign-out screen looks exactly like the button not working. */
        data class SignedOut(val problem: String? = null) : State
        data class SignedIn(val name: String?) : State
    }

    private val manager = CredentialManager.create(context)

    /** Held for the session. Google's ID tokens last about an hour; the app asks for
     * a new one when the server refuses this one, which is the only reliable signal
     * that it has gone stale. */
    private var token: String? = null

    val isConfigured: Boolean get() = BuildConfig.GOOGLE_WEB_CLIENT_ID.isNotBlank()

    fun current(): String? = token

    /**
     * A token to send, asking Google for one if this device does not have it yet.
     *
     * Without this a device that came up offline would stay tokenless until the app was
     * restarted: the quiet restore is attempted once at launch, and a failure there is
     * how somebody ends up signed in with nothing to prove it. Asking again on the
     * first request after signal returns is what turns that state back into a working
     * one -- and it is why a persistent auth problem eventually surfaces as a refusal
     * rather than hiding behind "Offline" for ever.
     */
    suspend fun tokenNow(): String? {
        token?.let { return it }
        if (!isRemembered) return null
        attempt(onlyAuthorized = true, quiet = true)
        return token
    }

    /**
     * A fresh token, because the server refused the one it was given.
     *
     * Google's ID tokens last about an hour, and nothing about holding one says when it
     * stopped being good -- a 401 is the only reliable signal, which is why this is
     * driven by the answer rather than by a timer.
     *
     * Before this existed, an expired token signed somebody out. Every hour. That is
     * the whole of "why does it keep asking me to sign in".
     */
    suspend fun renew(): String? {
        token = null
        if (!isRemembered) return null
        attempt(onlyAuthorized = true, quiet = true)
        return token
    }

    fun signOut() {
        token = null
        remembered.edit().clear().apply()
    }

    /**
     * Picks up an existing Google session without showing anything.
     *
     * `filterByAuthorizedAccounts` is true here: this is the quiet path, for somebody
     * who has signed in before. Failing it is not an error, it is a person who has
     * not.
     */
    suspend fun restore(): State = when (val asked = attempt(onlyAuthorized = true, quiet = true)) {
        // Google could not be asked, but somebody signed in on this phone and has not
        // signed out. Let them in to what is already on the device: every request will
        // fail as a transport error until there is signal, which is a state the app
        // already knows how to be in. Signing them out instead would hide their own
        // shopping behind a button that cannot work either.
        is State.SignedOut -> if (isRemembered) State.SignedIn(rememberedName) else asked
        else -> asked
    }

    /** The loud path, from a button: offers every account on the device. */
    suspend fun signIn(): State = attempt(onlyAuthorized = false, quiet = false)

    private suspend fun attempt(onlyAuthorized: Boolean, quiet: Boolean): State {
        if (!isConfigured) {
            return State.SignedOut(
                if (quiet) null else "This build has no Google client id."
            )
        }

        val option = GetGoogleIdOption.Builder()
            .setFilterByAuthorizedAccounts(onlyAuthorized)
            .setServerClientId(BuildConfig.GOOGLE_WEB_CLIENT_ID)
            .setAutoSelectEnabled(onlyAuthorized)
            .build()

        return try {
            val response = manager.getCredential(
                context,
                GetCredentialRequest.Builder().addCredentialOption(option).build(),
            )
            val credential = GoogleIdTokenCredential.createFrom(response.credential.data)
            token = credential.idToken
            remember(credential.displayName)
            State.SignedIn(credential.displayName)
        } catch (e: GetCredentialException) {
            // Restoring quietly is allowed to fail silently: it is the path for
            // somebody who has signed in before, and failing it just means they have
            // not. A tap on the button is not, and used to produce nothing at all.
            State.SignedOut(if (quiet) null else e.explain())
        }
    }
}

/**
 * What to tell somebody when Credential Manager refuses.
 *
 * The library's own messages are for developers -- `[28433]` and a stack of obfuscated
 * frames. These say what to do about it, and the commonest case by a distance is an
 * emulator with no Google account on it, where nothing is wrong with the app at all.
 */
private fun GetCredentialException.explain(): String = when (this) {
    is NoCredentialException ->
        "No Google account on this device. Add one in Settings, then try again."
    is GetCredentialCancellationException -> "Sign-in cancelled."
    else -> message ?: "Google would not sign you in."
}

private const val SIGNED_IN = "signed_in"
private const val NAME = "name"
