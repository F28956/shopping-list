import Foundation
import GoogleSignIn

/// Who is signed in, and the token to prove it.
///
/// The Google SDK holds the credential and refreshes it; this type is the thin part
/// that the rest of the app talks to. Refresh matters more here than in the browser:
/// a Google ID token lasts about an hour, and nobody is going to sign in again in the
/// middle of a shop.
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

    /// Whether the app was built with a client id at all.
    ///
    /// Without one the sign-in screen says so rather than failing at the tap: the
    /// value comes from `Config.xcconfig`, which is not committed, so a fresh clone
    /// genuinely has none.
    var isConfigured: Bool {
        clientID?.isEmpty == false
    }

    private var clientID: String? {
        Bundle.main.object(forInfoDictionaryKey: "GIDClientID") as? String
    }

    /// Picks up a session left from last time, without showing anything.
    func restore() async {
        guard isConfigured else {
            state = .signedOut
            return
        }

        do {
            let user = try await GIDSignIn.sharedInstance.restorePreviousSignIn()
            state = .signedIn(name: user.profile?.name)
        } catch {
            state = .signedOut
        }
    }

    func signIn(presenting: UIViewController) async {
        guard isConfigured else {
            lastError = "This build has no Google client id — see ios/README.md."
            return
        }

        do {
            let result = try await GIDSignIn.sharedInstance.signIn(withPresenting: presenting)
            state = .signedIn(name: result.user.profile?.name)
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    func signOut() {
        GIDSignIn.sharedInstance.signOut()
        state = .signedOut
    }

    /// A current ID token, refreshing it if it has expired.
    ///
    /// Returns nil rather than throwing: every caller's answer to "no token" is the
    /// same, and it is the sign-in screen.
    func token() async -> String? {
        guard let user = GIDSignIn.sharedInstance.currentUser else { return nil }

        do {
            let refreshed = try await user.refreshTokensIfNeeded()
            return refreshed.idToken?.tokenString
        } catch {
            return nil
        }
    }
}
