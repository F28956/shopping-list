package com.cernauskas.shoppinglist.data

import android.net.Uri

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
) {
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
         * A path, query or fragment — refused rather than silently dropped, because
         * dropping part of what somebody typed is how they end up at the wrong server
         * believing they are at the right one.
         */
        NOT_JUST_AN_ORIGIN,
        ;

        fun sentence(): String = when (this) {
            EMPTY -> "Enter the address of your Shopping List server."
            NOT_AN_ADDRESS -> "That does not look like an address."
            INSECURE -> "Addresses must start with https://"
            NOT_JUST_AN_ORIGIN -> "Enter just the address, with no path after it."
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
         * Refused: a path, a query or a fragment. Those are not beyond doubt.
         */
        fun parse(typed: String, allowingCleartext: Boolean): Result<ServerAddress> {
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
            if (scheme != "https" && !allowingCleartext) {
                return Result.failure(Refused(Problem.INSECURE))
            }

            // A lone trailing slash is what a browser's address bar shows and is not a
            // path anybody meant. Anything more is.
            val path = uri.path.orEmpty()
            if (path.isNotEmpty() && path != "/") {
                return Result.failure(Refused(Problem.NOT_JUST_AN_ORIGIN))
            }
            if (uri.query != null || uri.fragment != null) {
                return Result.failure(Refused(Problem.NOT_JUST_AN_ORIGIN))
            }

            // Reassembled rather than trimmed, so the stored form is the one this type
            // promises whatever arrived.
            val port = uri.port
            val origin = buildString {
                append(scheme).append("://").append(host)
                if (port != -1 && port != defaultPort(scheme)) append(':').append(port)
            }

            return Result.success(ServerAddress(origin))
        }

        private fun defaultPort(scheme: String) = if (scheme == "https") 443 else 80
    }

    /** Carries a [Problem] through `Result`, which wants a `Throwable`. */
    class Refused(val problem: Problem) : Exception(problem.sentence())
}

/** The [ServerAddress.Problem] behind a failure, for a screen that has to say something. */
val Throwable.addressProblem: ServerAddress.Problem?
    get() = (this as? ServerAddress.Refused)?.problem
