import Foundation
import Testing

@testable import ShoppingList

/// That the session token survives being put down.
///
/// These look like tests of Apple's keychain, and are not. They are tests of *this
/// build*: a keychain item is scoped by the `application-identifier` entitlement, and a
/// build signed without one gets `errSecMissingEntitlement` on every write. The symptom
/// is not an error anybody sees — it is an app that shows the sign-in screen again on
/// the next launch, having silently forgotten a session it just went and got.
///
/// That happened, which is why these exist. They run in the app's own process on a
/// simulator, so they fail exactly when the signing is wrong and need nobody's password
/// to say so.
struct KeychainTests {
    /// A name of this test's own, so a run cannot disturb a real signed-in session.
    private let key = "test.session.token"

    private func clear() {
        Keychain.set(nil, for: key)
    }

    @Test func aTokenComesBackTheWayItWentIn() {
        clear()
        defer { clear() }

        Keychain.set("a-token", for: key)

        #expect(
            Keychain.string(for: key) == "a-token",
            """
            The keychain would not hold a token. If this build has no signing identity \
            its entitlements are empty, and without application-identifier every write \
            fails — see ios/README.md.
            """
        )
    }

    /// Signing in as somebody else must not leave the last person's token behind.
    @Test func writingAgainReplacesRatherThanKeepingBoth() {
        clear()
        defer { clear() }

        Keychain.set("first", for: key)
        Keychain.set("second", for: key)

        #expect(Keychain.string(for: key) == "second")
    }

    /// Signing out has to actually remove it, or "sign out" clears a screen and
    /// nothing else.
    @Test func clearingLeavesNothingToRead() {
        Keychain.set("a-token", for: key)
        Keychain.set(nil, for: key)

        #expect(Keychain.string(for: key) == nil)
    }

    @Test func aNameNothingWasStoredUnderIsEmpty() {
        #expect(Keychain.string(for: "test.nothing.was.written.here") == nil)
    }
}
