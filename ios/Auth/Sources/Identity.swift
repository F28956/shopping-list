import AuthenticationServices
import Foundation

/// Who is signed in, and the token to prove it.
///
/// Sign in with Apple, and then not Apple again. Apple's identity token lasts about
/// ten minutes and has no silent refresh, so it is a bootstrap rather than a
/// credential: it is traded once, at `POST /api/sessions`, for a token this server
/// issued. That one lives in the keychain, lasts until it is unused for three months,
/// and needs no network to produce — which is exactly what an app that has to work in
/// a basement supermarket needs.
///
/// The consequence worth knowing: after the first sign-in, nothing here talks to Apple
/// at all. A phone that has been offline for a fortnight still has a working token.
///
/// Shared by the phone and the Mac. Not by the watch, which has no Sign in with Apple
/// and asks the phone instead — see `WatchIdentity`.
@MainActor
@Observable
final class Identity {
    enum State {
        case unknown
        case signedOut
        case signedIn(name: String?)
    }

    private(set) var state: State = .unknown
    private(set) var lastError: String?

    /// Kept so it can be removed again. `@ObservationIgnored` because it is
    /// bookkeeping: nothing draws it, and a view that redrew when it changed would be
    /// redrawing for no reason.
    @ObservationIgnored private var observer: (any NSObjectProtocol)?

    init() {
        watchForRevocation()
    }

    /// What is remembered between launches.
    ///
    /// The token is in the keychain; the name is in `UserDefaults`, because a display
    /// name grants nothing. Apple's own user identifier is deliberately not kept:
    /// nothing reads it now that revocation arrives as a notification rather than an
    /// answer to a question, and a stored identifier nothing consults is a thing to
    /// keep in step for no reason.
    private enum Remembered {
        static let token = "session.token"
        static let name = "session.name"
    }

    /// Whether somebody has signed in on this device and not signed out.
    ///
    /// Reads the keychain, so it is the truth rather than a flag alongside it: a
    /// device that has a token is signed in, and one that does not is not.
    var isRemembered: Bool { sessionToken != nil }

    private var sessionToken: String? {
        Keychain.string(for: Remembered.token)
    }

    private var rememberedName: String? {
        UserDefaults.standard.string(forKey: Remembered.name)
    }

    /// Whether this build can sign anybody in.
    ///
    /// Always, now. Sign in with Apple needs no client id and no configuration file —
    /// the entitlement is in the build and the audience is the bundle identifier — so
    /// there is no longer a "fresh clone has no credentials" state to report. Kept
    /// because the views ask, and because a build for a platform without Apple
    /// authentication would want to answer differently.
    var isConfigured: Bool { true }

    /// Picks up a session left from last time, without showing anything and without a
    /// network call.
    func restore() async {
        #if DEBUG
            // A UI test cannot sign in: the flow leaves the app and asks a person for
            // a passkey. It gets a signed-in person and a token that the stubbed wire
            // never checks; everything the test is actually about runs unchanged.
            if UITesting.isRunning {
                StubWorld.shared.reset(scenario: UITesting.scenario)
                state = .signedIn(name: "Test")
                return
            }
        #endif

        state = isRemembered ? .signedIn(name: rememberedName) : .signedOut
    }

    /// What to ask Apple for, handed to `SignInWithAppleButton`.
    ///
    /// The address is what admission reads and what links this person to the same
    /// person on Android; the name is what the lists are signed with. Both are given
    /// on the first authorisation and never again, on any device.
    func request(_ request: ASAuthorizationAppleIDRequest) {
        request.requestedScopes = [.email, .fullName]
    }

    /// Takes what Apple handed back and trades it for a session.
    func adopt(_ result: Result<ASAuthorization, Error>) async {
        switch result {
        case .success(let authorization):
            guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential
            else {
                lastError = "Apple returned something this app does not understand."
                return
            }
            await exchange(credential)

        case .failure(let error as ASAuthorizationError) where error.code == .canceled:
            // Not an error. Somebody changed their mind, and saying so in red is the
            // app arguing with a decision it was just handed.
            lastError = nil

        case .failure(let error):
            lastError = error.localizedDescription
        }
    }

    private func exchange(_ credential: ASAuthorizationAppleIDCredential) async {
        guard let data = credential.identityToken,
              let identityToken = String(data: data, encoding: .utf8)
        else {
            lastError = "Apple did not return an identity token."
            return
        }

        // The one and only time Apple sends a name: it is in the credential on the
        // first authorisation and never again. Stored now or lost.
        let name = credential.fullName.flatMap { parts -> String? in
            let formatted = PersonNameComponentsFormatter.localizedString(
                from: parts, style: .default
            )
            return formatted.isEmpty ? nil : formatted
        } ?? rememberedName

        do {
            let token = try await SessionExchange.open(with: identityToken)

            Keychain.set(token, for: Remembered.token)
            UserDefaults.standard.set(name, forKey: Remembered.name)

            state = .signedIn(name: name)
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    /// Signs out, optionally saying why.
    ///
    /// The reason lands in `lastError`, which the sign-in screen shows. That is where
    /// a refusal belongs: somebody this server will not admit is not somebody with an
    /// empty list, and leaving them on a Lists screen to be told about lists answers a
    /// question they did not ask. A plain sign-out clears any stale reason.
    ///
    /// The server is told, so the token stops working rather than merely being
    /// forgotten — but not waited for. A sign-out that hung until a shop's wifi
    /// answered would be a sign-out that did not work when it mattered most, and the
    /// token is gone from this device either way.
    func signOut(because reason: String? = nil) {
        if let token = sessionToken {
            Task.detached { await SessionExchange.close(token) }
        }

        Keychain.set(nil, for: Remembered.token)
        UserDefaults.standard.removeObject(forKey: Remembered.name)
        state = .signedOut
        lastError = reason
    }

    /// The token to put in a request.
    ///
    /// Synchronous in everything but signature: it is a keychain read. There is no
    /// refresh to attempt and no provider to wait for, which is the whole point of
    /// having exchanged Apple's token for this one.
    func token() async -> String? {
        #if DEBUG
            if UITesting.isRunning { return "ui-test-token" }
        #endif

        return sessionToken
    }

    /// Stops listening. Paired with `watchForRevocation`, and only so that a torn-down
    /// `Identity` does not leave an observer behind.
    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    /// Signs out when somebody takes this app off their Apple ID.
    ///
    /// A notification, not a poll. `credentialState(forUserID:)` looked like the
    /// obvious thing and was actively harmful: asked at every launch it answered
    /// `.revoked` for an account that was fine, and the app dutifully signed itself out
    /// sixteen seconds after signing in. A wrong `.revoked` costs somebody their
    /// session and anything still queued in the outbox behind it, so the only news
    /// worth acting on is news that arrives by itself.
    ///
    /// The server is the other half, and the authoritative one. It knows about
    /// admission and about sessions ended elsewhere, and it says so with a 401 — which
    /// is what the wire already handles.
    private func watchForRevocation() {
        observer = NotificationCenter.default.addObserver(
            forName: ASAuthorizationAppleIDProvider.credentialRevokedNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.signOut(because: "This Apple ID no longer allows Shopping list.")
            }
        }
    }

}

/// `POST /api/sessions` and its undo.
///
/// Deliberately not part of `API`: that type takes a token provider, and this is what
/// produces the token it would be asking for.
private enum SessionExchange {
    private struct Issued: Decodable {
        let token: String
    }

    static func open(with identityToken: String) async throws -> String {
        var request = URLRequest(url: Config.apiBaseURL.appendingPathComponent("api/sessions"))
        request.httpMethod = "POST"
        request.setValue("Bearer \(identityToken)", forHTTPHeaderField: "Authorization")

        let (data, response) = try await URLSession.shared.data(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0

        switch code {
        case 200:
            return try JSONDecoder().decode(Issued.self, from: data).token
        case 403:
            // Admission, not authorisation. Said plainly, because "forbidden" on a
            // sign-in screen reads as a bug in the app rather than a decision about
            // the account.
            throw APIError.badInput("This account is not allowed on this server.")
        default:
            throw APIError.server(code)
        }
    }

    /// Best effort. The token is gone from this device whatever the server says.
    static func close(_ token: String) async {
        var request = URLRequest(url: Config.apiBaseURL.appendingPathComponent("api/sessions"))
        request.httpMethod = "DELETE"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

        _ = try? await URLSession.shared.data(for: request)
    }
}
