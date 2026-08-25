import Foundation
import WatchConnectivity
import os

/// The watch's credential, which is really the phone's.
///
/// There is no sign-in here and there cannot be: Google's SDK has no watchOS build,
/// and a watch has no browser to run the flow in. This asks the phone each time and
/// caches the answer only for as long as it is plausibly good.
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

    private var cached: String?
    /// Google ID tokens last about an hour. Well inside that, so a token is never
    /// handed to a request that is about to be refused for being stale.
    private var cachedUntil: Date = .distantPast
    private static let cacheFor: TimeInterval = 30 * 60

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

    /// A token to put in a request, or nil if the phone could not supply one.
    func token() async -> String? {
        if let cached, cachedUntil > Date() { return cached }

        guard let fresh = await ask() else {
            state = .unavailable
            return nil
        }

        cached = fresh
        cachedUntil = Date().addingTimeInterval(Self.cacheFor)
        state = .ready
        return fresh
    }

    /// Drops the cached token, so the next request fetches a new one.
    ///
    /// Called when the server refuses one: the cache window is a guess about expiry,
    /// and a 401 is the server telling us the guess was wrong.
    func refused() {
        cached = nil
        cachedUntil = .distantPast
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
