import Foundation
import GoogleSignIn

#if canImport(UIKit)
    import UIKit

    /// What the sign-in sheet is presented from. A view controller on a phone, a
    /// window on a Mac — the flow either side of it is identical, which is why this
    /// file is shared rather than written twice.
    typealias SignInHost = UIViewController
#elseif canImport(AppKit)
    import AppKit

    typealias SignInHost = NSWindow
#endif

/// Who is signed in, and the token to prove it.
///
/// The Google SDK holds the credential and refreshes it; this type is the thin part
/// that the rest of the app talks to. Refresh matters more here than in the browser:
/// a Google ID token lasts about an hour, and nobody is going to sign in again in the
/// middle of a shop.
///
/// Shared by the phone and the Mac. Not by the watch, which cannot sign in at all and
/// does not link the Google SDK — see `WatchIdentity`.
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

    /// That somebody is signed in on this device, and what to call them.
    ///
    /// Kept because refreshing a Google token needs a connection, and a phone opened in
    /// a shop with no signal would otherwise show the sign-in screen with the cached
    /// list stranded behind it — which is the case this whole piece of work is about.
    /// What is remembered is a flag and a display name: no token, nothing that grants
    /// anything, and nothing that outlives signing out.
    private enum Remembered {
        static let signedIn = "session.signedIn"
        static let name = "session.name"
    }

    /// Whether somebody has signed in on this device and not signed out.
    var isRemembered: Bool { UserDefaults.standard.bool(forKey: Remembered.signedIn) }

    private var rememberedName: String? {
        get { UserDefaults.standard.string(forKey: Remembered.name) }
        set {
            UserDefaults.standard.set(true, forKey: Remembered.signedIn)
            UserDefaults.standard.set(newValue, forKey: Remembered.name)
        }
    }

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

        guard isConfigured else {
            state = .signedOut
            return
        }

        do {
            let user = try await GIDSignIn.sharedInstance.restorePreviousSignIn()
            rememberedName = user.profile?.name
            state = .signedIn(name: user.profile?.name)
        } catch {
            // Google could not be asked, but somebody signed in on this device and has
            // not signed out. Let them in to what is already here: every request will
            // fail as a transport error until there is signal, which is a state the app
            // already knows how to be in. Signing them out instead would hide their own
            // shopping behind a button that cannot work either.
            state = isRemembered ? .signedIn(name: rememberedName) : .signedOut
        }
    }

    func signIn(presenting: SignInHost) async {
        guard isConfigured else {
            lastError = "This build has no Google client id — see ios/README.md."
            return
        }

        do {
            let result = try await GIDSignIn.sharedInstance.signIn(withPresenting: presenting)
            rememberedName = result.user.profile?.name
            state = .signedIn(name: result.user.profile?.name)
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
    func signOut(because reason: String? = nil) {
        GIDSignIn.sharedInstance.signOut()
        UserDefaults.standard.removeObject(forKey: Remembered.signedIn)
        UserDefaults.standard.removeObject(forKey: Remembered.name)
        state = .signedOut
        lastError = reason
    }

    /// A current ID token, refreshing it if it has expired.
    ///
    /// Returns nil rather than throwing: every caller's answer to "no token" is the
    /// same, and it is the sign-in screen.
    func token() async -> String? {
        #if DEBUG
            if UITesting.isRunning { return "ui-test-token" }
        #endif

        guard let user = GIDSignIn.sharedInstance.currentUser else { return nil }

        do {
            let refreshed = try await user.refreshTokensIfNeeded()
            return refreshed.idToken?.tokenString
        } catch {
            return nil
        }
    }
}
