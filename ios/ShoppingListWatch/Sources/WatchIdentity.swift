import Foundation
import WatchConnectivity
import os

/// The watch's credential, which is really the phone's.
///
/// There is no sign-in here and there cannot be: watchOS has no Sign in with Apple
/// sheet and a watch has no browser to run a flow in. So it asks the phone, over a
/// link Apple has already authenticated, and keeps what it is given.
///
/// Kept in the keychain rather than in memory, and kept until it stops working rather
/// than for a guessed few minutes. What the phone hands over is a session token this
/// server issued, good for ninety days of use — so a watch that has been near its
/// phone once goes on working in a shop with no signal and a phone left at home,
/// which is the whole reason the watch has a cache and an outbox at all.
@MainActor
@Observable
final class WatchIdentity: NSObject, WCSessionDelegate {
    enum State: Equatable {
        case unknown
        /// The phone has not been reachable, or is not signed in.
        case unavailable
        case ready
    }

    private(set) var state: State = .unknown

    private static let stored = "session.token"

    private var cached: String? {
        get { WatchKeychain.string(for: Self.stored) }
        set { WatchKeychain.set(newValue, for: Self.stored) }
    }

    override init() {
        super.init()
        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()
    }

    nonisolated func session(
        _ session: WCSession,
        activationDidCompleteWith activation: WCSessionActivationState,
        error: Error?
    ) {}

    /// A token to put in a request, or nil if there is none and the phone could not
    /// supply one.
    ///
    /// The stored one is used without asking. Nothing here can tell whether it is
    /// still good, and the server can — so the check is a 401, which `refused()`
    /// handles, rather than a clock this side guessing at one.
    func token() async -> String? {
        if let cached {
            state = .ready
            return cached
        }

        guard let fresh = await ask() else {
            state = .unavailable
            return nil
        }

        cached = fresh
        state = .ready
        return fresh
    }

    /// Throws the token away, because the server said no.
    ///
    /// A 401 is the only reliable news that a session has ended — revoked on the
    /// phone, or idle past ninety days. The next request asks the phone for a new one,
    /// and gets nowhere until the phone is in range, which is the honest state to be
    /// in: without a credential there is nothing this watch can send.
    func refused() {
        cached = nil
        state = .unknown
    }

    private func ask() async -> String? {
        let session = WCSession.default
        guard session.activationState == .activated, session.isReachable else { return nil }

        return await withCheckedContinuation { continuation in
            // `resume` exactly once: WatchConnectivity calls one handler or the other,
            // but a continuation resumed twice is a crash rather than a bug report.
            let answered = OSAllocatedUnfairLock(initialState: false)
            func finish(_ token: String?) {
                let first = answered.withLock { done -> Bool in
                    defer { done = true }
                    return !done
                }
                if first { continuation.resume(returning: token) }
            }

            session.sendMessage(
                [WatchLink.tokenRequest: true],
                replyHandler: { reply in
                    finish(reply[WatchLink.tokenRequest] as? String)
                },
                errorHandler: { _ in finish(nil) }
            )
        }
    }
}
