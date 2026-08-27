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
    private static let storedServer = "session.server"

    private var cached: String? {
        get { WatchKeychain.string(for: Self.stored) }
        set { WatchKeychain.set(newValue, for: Self.stored) }
    }

    /// Which server that token is for.
    ///
    /// Beside the token and cleared with it, because they are useless apart: sending
    /// one server's token to another is at best a refusal and at worst a credential
    /// handed to a stranger. Kept in the keychain rather than `UserDefaults` for no
    /// reason other than that it lives and dies with the thing that is.
    private var cachedServer: String? {
        get { WatchKeychain.string(for: Self.storedServer) }
        set { WatchKeychain.set(newValue, for: Self.storedServer) }
    }

    /// Where to send requests, or `nil` if this watch has never heard from its phone.
    var serverAddress: URL? {
        cachedServer.flatMap { URL(string: $0) }
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
        // Both or neither. A token with no address is a credential with nowhere safe
        // to send it, so a half-answer is treated as no answer.
        if let cached, cachedServer != nil {
            state = .ready
            return cached
        }

        guard let (token, server) = await ask() else {
            state = .unavailable
            return nil
        }

        cached = token
        cachedServer = server
        // Where `Config.apiBaseURL` reads from, so the rest of the watch app needs to
        // know nothing about how the address arrived.
        if case .success(let address) = ServerAddress.parse(server, allowingCleartext: true) {
            ServerDirectory.remember(address)
        }
        state = .ready
        return token
    }

    /// Throws the token away, because the server said no.
    ///
    /// A 401 is the only reliable news that a session has ended — revoked on the
    /// phone, or idle past ninety days. The next request asks the phone for a new one,
    /// and gets nowhere until the phone is in range, which is the honest state to be
    /// in: without a credential there is nothing this watch can send.
    func refused() {
        cached = nil
        cachedServer = nil
        state = .unknown
    }

    private func ask() async -> (token: String, server: String)? {
        let session = WCSession.default
        guard session.activationState == .activated, session.isReachable else { return nil }

        return await withCheckedContinuation { (continuation: CheckedContinuation<(token: String, server: String)?, Never>) in
            // `resume` exactly once: WatchConnectivity calls one handler or the other,
            // but a continuation resumed twice is a crash rather than a bug report.
            let answered = OSAllocatedUnfairLock(initialState: false)
            func finish(_ answer: (token: String, server: String)?) {
                let first = answered.withLock { done -> Bool in
                    defer { done = true }
                    return !done
                }
                if first { continuation.resume(returning: answer) }
            }

            session.sendMessage(
                [WatchLink.tokenRequest: true],
                replyHandler: { reply in
                    guard let token = reply[WatchLink.tokenRequest] as? String,
                          let server = reply[WatchLink.serverAddress] as? String
                    else {
                        finish(nil)
                        return
                    }
                    finish((token, server))
                },
                errorHandler: { _ in finish(nil) }
            )
        }
    }
}
