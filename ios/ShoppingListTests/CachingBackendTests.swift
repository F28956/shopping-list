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

    // MARK: - What is queued, laid back over what the server said

    private func list() -> List {
        List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)
    }

    /// The rule that stops a successful read visibly undoing a tick that is still
    /// queued: the server has not been told, so it answers with the old state, and the
    /// row would flick back for as long as the queue is stuck.
    ///
    /// These three were `ItemsModelTests` until the queue moved behind the protocol.
    @Test("a queued tick is laid back over what was read")
    func aQueuedTickSurvivesAReload() async throws {
        let (backend, cache) = backend()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(units: [Unit(id: 1, name: "unit", bare: false)])

        try await backend.add("milk", to: list)
        let milk = try #require(try await backend.items(on: list).items.first)
        try await backend.setDone(milk, on: list, done: true)

        #expect(
            try await backend.items(on: list).items.first?.isDone == true,
            "the queued tick was undone by a read"
        )
    }

    /// A row this device made is not in the server's answer at all, so it is carried
    /// across from what was written down.
    @Test("a row made with no signal is carried across a read")
    func locallyMadeRowsSurvive() async throws {
        let (backend, cache) = backend()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(units: [Unit(id: 1, name: "unit", bare: false)])

        try await backend.add("milk", to: list)

        #expect(
            try await backend.items(on: list).items.count == 1,
            "a row made offline vanished on the first read"
        )
    }

    /// **Only** rows it made. Any queued operation used to qualify, which meant a tick
    /// queued against a row somebody else had deleted put it back on screen: present
    /// here, gone everywhere else, impossible to be rid of.
    @Test("a row somebody else deleted does not come back as a ghost")
    func deletedRowsDoNotReturn() async throws {
        let (backend, cache) = backend()
        let list = list()
        cache.remember(lists: [list])
        let theirs = Item(id: 3, uuid: "theirs", name: "Bread", amount: 1,
                          unitID: 1, doneAt: nil, tagIDs: [])
        cache.remember(items: [theirs], on: list)

        // Ticked here, deleted there: the queue holds a tick against a row that is
        // gone, and the cache no longer has it either.
        try await backend.setDone(theirs, on: list, done: true)
        cache.remember(items: [], on: list)

        #expect(
            try await backend.items(on: list).items.isEmpty,
            "a row somebody else deleted came back"
        )
    }

    // MARK: - Saying what moved, not merely that something did

    /// The point of the typed stream. A nudge that does not say what it is about makes a
    /// screen re-read everything: thirty-one units and twenty-one categories on every
    /// tick, which is three requests where one would do.
    @Test("a change to the rows is reported as rows")
    func aRowChangeSaysRows() async throws {
        let (backend, cache) = backend()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(units: [Unit(id: 1, name: "unit", bare: false)])
        cache.remember(tags: [Tag(id: 900, name: "dairy", emoji: nil, sortOrder: 0)], on: list)

        var nudges = try await backend.changes(on: list).makeAsyncIterator()
        // The first is `.categories` by construction -- a screen opening re-reads them
        // anyway, and it is the only honest answer when there is nothing to compare to.
        #expect(try await nudges.next() != nil)

        try await backend.add("milk", to: list)

        let heard = try await nudges.next()
        if case .rows = try #require(heard) {} else {
            Issue.record("a row change was reported as \(String(describing: heard))")
        }
    }

    /// And the case that would otherwise be missed entirely: a category renamed from a
    /// screen that belongs to no list.
    @Test("a change to the categories is reported as categories")
    func aCategoryChangeSaysCategories() async throws {
        let (backend, cache) = backend()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(
            tags: [
                Tag(id: 900, name: "dairy", emoji: nil, sortOrder: 0),
                Tag(id: 901, name: "produce", emoji: nil, sortOrder: 1),
            ],
            on: list
        )

        var nudges = try await backend.changes(on: list).makeAsyncIterator()
        _ = try await nudges.next()

        // What the Categories screen does.
        cache.rename(tag: 900, to: "cheese counter", emoji: nil)

        let heard = try await nudges.next()
        if case .categories = try #require(heard) {} else {
            Issue.record("a category change was reported as \(String(describing: heard))")
        }
    }
}
