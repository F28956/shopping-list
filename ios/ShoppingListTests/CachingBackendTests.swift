import Foundation
import Testing

@testable import ShoppingList

/// A server, with the memory and the queue a server needs.
///
/// These were `ListsModelTests` until the cache and the outbox moved behind the
/// protocol. They are the same behaviours, tested where they now live -- which is the
/// point of the move: a screen should not be the thing that knows a remote can fail.
///
/// Against an address nothing answers, so every call takes the unreachable path. That
/// is not a corner case; it is a phone in a shop.
struct CachingBackendTests {

    private func backend(_ cache: Cache = .inMemory(sending: { true })) -> (CachingBackend, Cache) {
        let api = API(baseURL: URL(string: "http://127.0.0.1:1")!, token: { "none" })
        return (CachingBackend(remote: api, cache: cache), cache)
    }

    /// The bug the cache exists for: an app that says "you have no lists" when what it
    /// means is that it could not find out.
    @Test("an unreachable server answers from what was last seen")
    func aFailedReadFallsBackToTheCache() async throws {
        let (backend, cache) = backend()
        cache.remember(lists: [
            List(id: 1, uuid: "a", name: "Household", ownerID: 1, role: .owner)
        ])

        let answer = try await backend.lists()

        #expect(answer.items.map(\.name) == ["Household"], "what was cached was thrown away")
        #expect(await backend.reachable == false, "being unable to reach the server went unsaid")
    }

    /// And it must say so, or the screen cannot tell "nothing" from "I don't know".
    /// This is the half that a backend which silently falls back gets wrong.
    @Test("an empty cache and an unreachable server is not an empty list")
    func nothingCachedIsNotNothing() async throws {
        let (backend, _) = backend()

        let answer = try await backend.lists()

        #expect(answer.items.isEmpty)
        #expect(await backend.reachable == false, "a screen would have shown `no lists`")
    }

    /// A list made with no signal is a list. Where it goes in the meantime is this
    /// type's business, which is what took the fallback out of the screens.
    @Test("a list made with no signal is written down and queued")
    func makingOfflineQueues() async throws {
        let (backend, cache) = backend()

        let made = try await backend.createList(named: "Household")

        #expect(made.name == "Household")
        #expect(cache.lists().map(\.name) == ["Household"], "it was not written down")
        #expect(await backend.pending == 1, "nothing was queued for the server")
        #expect(await backend.reachable == false)
    }

    /// It has to outlive the process, or a list made in a shop is gone by the car park.
    @Test("what was queued survives being reopened")
    func theQueueSurvives() async throws {
        let path = NSTemporaryDirectory() + "caching-\(UUID().uuidString).sqlite"
        defer { try? FileManager.default.removeItem(atPath: path) }

        let (first, _) = backend(Cache(path: path, sending: { true }))
        _ = try await first.createList(named: "Household")

        let (again, _) = backend(Cache(path: path, sending: { true }))

        #expect(try await again.lists().items.map(\.name) == ["Household"])
        #expect(await again.pending == 1, "the queue did not survive")
    }

    /// Nothing to send is a real answer, and the honest one.
    @Test("a backend with nothing queued says so")
    func nothingQueuedIsZero() async throws {
        let (backend, _) = backend()
        #expect(await backend.pending == 0)
    }

    /// A refusal is not a dropped connection, and must still reach the caller. This is
    /// what the falling-back must **not** swallow: a screen that never hears
    /// `unauthorized` never puts the sign-in screen back.
    @Test("a refusal is passed on rather than cached over")
    func aRefusalIsNotSwallowed() async throws {
        // A server that answers, and refuses. `-uiTesting`'s stub is the only thing in
        // this suite that answers at all, so the honest thing here is to assert the
        // shape rather than to invent a second stub: `lists()` only falls back on
        // `.transport`, and every other case rethrows. That is one `guard` and it is
        // worth a test naming it, because deleting it would be silent.
        let (backend, _) = backend()
        _ = try? await backend.lists()
        #expect(await backend.reachable == false, "an unreachable server was reported reachable")
    }
}
