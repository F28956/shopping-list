package com.cernauskas.shoppinglist.data

import android.content.Context
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.exceptions.GetCredentialException
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

    sealed interface State {
        data object Unknown : State
        data object SignedOut : State
        data class SignedIn(val name: String?) : State
    }

    private val manager = CredentialManager.create(context)

    /** Held for the session. Google's ID tokens last about an hour; the app asks for
     * a new one when the server refuses this one, which is the only reliable signal
     * that it has gone stale. */
    private var token: String? = null

    val isConfigured: Boolean get() = BuildConfig.GOOGLE_WEB_CLIENT_ID.isNotBlank()

    fun current(): String? = token

    /** Forgets the token, so the next request asks for a fresh one. Called when the
     * server refuses one: the age of a token is a guess, and a 401 is the server
     * saying the guess was wrong. */
    fun refused() {
        token = null
    }

    fun signOut() {
        token = null
    }

    /**
     * Picks up an existing Google session without showing anything.
     *
     * `filterByAuthorizedAccounts` is true here: this is the quiet path, for somebody
     * who has signed in before. Failing it is not an error, it is a person who has
     * not.
     */
    suspend fun restore(): State = attempt(onlyAuthorized = true)

    /** The loud path, from a button: offers every account on the device. */
    suspend fun signIn(): State = attempt(onlyAuthorized = false)

    private suspend fun attempt(onlyAuthorized: Boolean): State {
        if (!isConfigured) return State.SignedOut

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
            State.SignedIn(credential.displayName)
        } catch (e: GetCredentialException) {
            State.SignedOut
        }
    }
}
