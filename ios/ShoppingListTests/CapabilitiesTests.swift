import Foundation
import Testing

@testable import ShoppingList

/// What the app offers, as opposed to how it fetches things.
///
/// Small, and worth having: these three booleans agree today and are named apart on
/// purpose, so a site that hides something says *why*. A test that only checked they
/// move together would defeat the point of separating them.
struct CapabilitiesTests {

    @Test("a device with a server offers everything")
    func withAServer() {
        let can = Capabilities.withAServer
        #expect(can.sharing)
        #expect(can.accounts)
        #expect(can.syncing)
    }

    /// Not a degraded mode. A shopping list is meant to be useful the moment it is
    /// installed, and what is absent is absent because it would be a lie: there is
    /// nobody to share with, no account, and no far end to be out of reach of.
    @Test("a device on its own offers none of the three")
    func onItsOwn() {
        let can = Capabilities.onItsOwn
        #expect(!can.sharing)
        #expect(!can.accounts)
        #expect(!can.syncing)
    }

    /// The combination the separation exists for, and the reason this is three
    /// booleans rather than one.
    ///
    /// A list shared device-to-device, with no server anywhere: somebody to share with,
    /// and still no account. The flag this replaced could not say that -- `onDeviceOnly`
    /// hid sharing and accounts together, so the day sharing arrives without a server
    /// every one of those sites would have to be found and re-read.
    @Test("sharing without an account is expressible")
    func sharingWithoutAnAccount() {
        let peerToPeer = Capabilities(sharing: true, accounts: false, syncing: true)

        #expect(peerToPeer.sharing, "there is somebody to share with")
        #expect(!peerToPeer.accounts, "and still nobody to sign in as")
        #expect(peerToPeer != .withAServer)
        #expect(peerToPeer != .onItsOwn)
    }

    /// What a device answers depends on whether a server has been chosen, and nothing
    /// else -- no network check, because whether a server is *reachable* is the
    /// backend's question and a different one.
    @Test("what a device offers follows whether a server was chosen")
    func currentFollowsTheChoice() {
        let key = "server.address"
        let before = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(before, forKey: key) }

        UserDefaults.standard.set("", forKey: key)
        #expect(Capabilities.current == .onItsOwn)

        UserDefaults.standard.set("https://shopping.example.com", forKey: key)
        #expect(Capabilities.current == .withAServer)
    }
}
