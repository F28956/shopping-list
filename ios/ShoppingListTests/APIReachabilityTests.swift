import Foundation
import Testing

@testable import ShoppingList

/// Whether an `API` thinks it has anywhere to send.
///
/// One question, asked two ways, and conflating them cost a watch every request it
/// made. A phone is pointed at `Config.apiBaseURL` whether or not anybody has chosen a
/// server, so *it* must consult this device's stored choice. A watch is handed an
/// address by its phone and stores nothing of its own -- so asking the same global
/// answers "no server" while the address sits in the object.
struct APIReachabilityTests {

    private func noServerStored<T>(_ body: () async throws -> T) async rethrows -> T {
        let key = "server.address"
        let before = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(before, forKey: key) }
        UserDefaults.standard.set("", forKey: key)
        return try await body()
    }

    /// The phone and the Mac: no server chosen means every call fails before a socket
    /// is opened, which is what makes "no server" and "no signal" one state.
    @Test("a fixed address with no server chosen has nowhere to send")
    func aFixedAddressFollowsTheStoredChoice() async throws {
        try await noServerStored {
            let api = API(baseURL: URL(string: "https://shopping.example.com")!, token: { "t" })

            await #expect(throws: (any Error).self) { _ = try await api.lists() }
        }
    }

    /// The watch: given an address, it has somewhere to send, whatever this device's
    /// own preferences happen to say.
    ///
    /// The regression this exists for: a watch whose phone had told it about a server
    /// built an `API` pointed at it and then refused every request with "This device is
    /// not using a server". On a list with cached rows the error was hidden behind
    /// them; on an empty one it was the whole screen, which is how it was found.
    @Test("an address handed over is somewhere to send, whatever this device stores")
    func ahandedAddressIsEnough() async throws {
        try await noServerStored {
            // Nothing answers on this port, so the failure must be a *transport* one --
            // a socket that could not be opened -- rather than the refusal that comes
            // before one.
            let api = API(server: { URL(string: "http://127.0.0.1:1")! }, token: { "t" })

            do {
                _ = try await api.lists()
                Issue.record("a request against a dead port succeeded")
            } catch let problem as APIError {
                guard case .transport(let underlying) = problem else {
                    Issue.record("refused rather than attempted: \(problem)")
                    return
                }
                #expect(
                    !(underlying is NoServer),
                    "the watch was told it has no server while holding an address"
                )
            }
        }
    }
}
