package com.cernauskas.shoppinglist.data

import android.net.Uri
import com.cernauskas.shoppinglist.BuildConfig

/**
 * Where the server is, as a thing that has been checked rather than a string.
 *
 * A self-hosted app has to ask, because the answer is different for everybody. What it
 * must not do is take what was typed and hope: a person pastes what is in their
 * browser's address bar, which has a path on it, and the failure that causes is silent
 * and much later.
 *
 * The rules here are the same ones `ios/Shared/Sources/ServerAddress.swift` applies,
 * deliberately and to the letter. Two clients that disagree about what an address means
 * are two clients that talk to different servers from the same typing.
 */
data class ServerAddress(
    /** `scheme://host` or `scheme://host:port`, lowercased, with no trailing slash. */
    val origin: String,
    /**
     * Where the server is mounted under that origin: `""` at the root, or a path
     * beginning with `/` and not ending in one.
     *
     * One domain often has several things behind it, and a server at
     * `https://example.com/sl` is an ordinary arrangement rather than a mistake --
     * insisting on a whole host would be a constraint on somebody's DNS. The server
     * end of this is `BASE_PATH`.
     */
    val prefix: String = "",
) {
    /** The address as somebody would type it, as it is stored, and as requests use it. */
    val written: String get() = origin + prefix

    /** Why an address could not be used, in the words the screen says. */
    enum class Problem {
        EMPTY,
        NOT_AN_ADDRESS,

        /**
         * C6. Release builds accept `https://` only: an app that can be pointed
         * anywhere and permits cleartext puts somebody's shopping and their bearer
         * token on whatever cafe Wi-Fi they are on.
         */
        INSECURE,

        /**
         * A query or fragment — refused rather than silently dropped, because
         * dropping part of what somebody typed is how they end up at the wrong server
         * believing they are at the right one.
         *
         * A *path* is no longer refused: it is kept, as the prefix the server is
         * mounted under. See [ServerAddress.prefix].
         */
        NOT_JUST_AN_ORIGIN,
        ;

        fun sentence(): String = when (this) {
            EMPTY -> "Enter the address of your Shopping List server."
            NOT_AN_ADDRESS -> "That does not look like an address."
            INSECURE -> "Addresses must start with https://"
            NOT_JUST_AN_ORIGIN -> "Enter the address without a ? query or # fragment."
        }
    }

    companion object {
        /**
         * Reads what somebody typed, repairing the obvious and refusing the ambiguous.
         *
         * Repaired: a missing scheme becomes `https://`, a trailing slash goes, the host
         * is lowercased, surrounding whitespace is ignored. All of those are what the
         * person meant beyond doubt.
         *
         * Kept: a path, which is the prefix the server is mounted under.
         *
         * Refused: a query or a fragment. Those are not beyond doubt.
         *
         * There is deliberately **no way to ask for cleartext**. There used to be a
         * parameter for it and every caller but one passed `true` — including the one
         * that reads a host out of a pasted share link, which is untrusted text from
         * whoever sent it. The release guarantee then held only because the single path
         * that stores an address happened to pass the flag. An invariant that depends
         * on four callers agreeing is not an invariant.
         *
         * Nothing is lost by removing it: a debug build allows cleartext through
         * [allowsCleartext] anyway, which is the case the parameter existed for, and a
         * release build now refuses `http://` everywhere including from a pasted link.
         */
        fun parse(typed: String): Result<ServerAddress> {
            val trimmed = typed.trim()
            if (trimmed.isEmpty()) return Result.failure(Refused(Problem.EMPTY))

            // Decided before parsing rather than repaired after: `Uri` reads
            // "example.com:8080" as the scheme `example.com`.
            val withScheme = if (trimmed.contains("://")) trimmed else "https://$trimmed"

            val uri = Uri.parse(withScheme)
            val scheme = uri.scheme?.lowercase()
            val host = uri.host?.lowercase()

            if (scheme == null || host.isNullOrEmpty()) {
                return Result.failure(Refused(Problem.NOT_AN_ADDRESS))
            }
            if (scheme != "https" && scheme != "http") {
                return Result.failure(Refused(Problem.NOT_AN_ADDRESS))
            }
            if (scheme != "https" && !allowsCleartext()) {
                return Result.failure(Refused(Problem.INSECURE))
            }

            if (uri.query != null || uri.fragment != null) {
                return Result.failure(Refused(Problem.NOT_JUST_AN_ORIGIN))
            }

            // A lone trailing slash is what a browser's address bar shows and is not a
            // path anybody meant. Anything more is the prefix the server is mounted
            // under, kept without a trailing slash so that `written + "/api/lists"` has
            // one slash between the two and never two.
            val path = uri.path.orEmpty()
            val prefix = if (path == "/") "" else path.trimEnd('/')

            // Reassembled rather than trimmed, so the stored form is the one this type
            // promises whatever arrived.
            val port = uri.port
            val origin = buildString {
                append(scheme).append("://").append(host)
                if (port != -1 && port != defaultPort(scheme)) append(':').append(port)
            }

            return Result.success(ServerAddress(origin, prefix))
        }

        private fun defaultPort(scheme: String) = if (scheme == "https") 443 else 80

        /**
         * C6: cleartext in debug builds, where the server is on the same desk, and
         * never in a release one.
         *
         * The manifest's `networkSecurityConfig` is the other half — this refuses the
         * address, and that refuses the connection.
         */
        fun allowsCleartext(): Boolean = BuildConfig.DEBUG
    }

    /** Carries a [Problem] through `Result`, which wants a `Throwable`. */
    class Refused(val problem: Problem) : Exception(problem.sentence())
}

/** The [ServerAddress.Problem] behind a failure, for a screen that has to say something. */
val Throwable.addressProblem: ServerAddress.Problem?
    get() = (this as? ServerAddress.Refused)?.problem
