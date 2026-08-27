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

    /// What is remembered between launches.
    ///
    /// The token is in the keychain; the rest is in `UserDefaults`, because none of it
    /// grants anything. The Apple user identifier is kept so that a revoked
    /// authorisation can be noticed — it is a per-app opaque string, not an address.
    private enum Remembered {
        static let token = "session.token"
        static let name = "session.name"
        static let appleUserID = "session.appleUserID"
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

        // Somebody who removed this app from their Apple ID, or stopped using that
        // Apple ID, should not stay signed in for the rest of the session's ninety
        // days. Checked after the state is set rather than before it, because the
        // answer needs the device to be awake and reachable and the list on screen
        // does not.
        await forgetIfRevoked()
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
            UserDefaults.standard.set(credential.user, forKey: Remembered.appleUserID)

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
        UserDefaults.standard.removeObject(forKey: Remembered.appleUserID)
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

    /// Signs out if this Apple ID no longer authorises this app.
    ///
    /// Only on a definite `.revoked`. `credentialState` also answers `.notFound` for
    /// an app it has no record of, and a device with no signal returns an error — and
    /// neither is a reason to throw away a working session.
    private func forgetIfRevoked() async {
        guard let user = UserDefaults.standard.string(forKey: Remembered.appleUserID),
              isRemembered
        else { return }

        let provider = ASAuthorizationAppleIDProvider()
        let credentialState = try? await provider.credentialState(forUserID: user)

        if credentialState == .revoked {
            signOut(because: "This Apple ID no longer allows Shopping list.")
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
