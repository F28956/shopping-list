// See `LocalServer` for why this is absent on the watch.
#if !os(watchOS)

import Foundation

/// A remote ``Backend``, with the memory and the queue a remote needs.
///
/// The third conformer, and the one that ends a fork. `LocalBackend` needs neither a
/// cache nor an outbox, because a database on this device cannot fail to answer.
/// `API` needs both, because a server can. That difference used to live in
/// `ListsModel` as `keepsItsOwnStore`, deciding three things: whether to write the
/// cache, whether a failure falls back to queueing, and whether there is anything to
/// drain.
///
/// All three are about *the far end being far*, not about which mode the app is in. So
/// they belong to the thing that talks to the far end. With this in place a model is
/// handed a `Backend` and asks nothing about it, which is what the protocol was drawn
/// for.
///
/// ## What it promises
///
/// Reads answer from the cache rather than failing, and writes are queued rather than
/// lost. A caller therefore sees far fewer errors than it did -- which is the point,
/// and also the risk: "I could not reach the server" must not become invisible. It
/// does not, because ``reachable`` says so, and the screens read it for the dot and
/// for the difference between "you have no lists" and "I could not find out".
actor CachingBackend {

    private let remote: any Backend & Destination
    private let cache: Cache

    /// Whether the last attempt to reach the far end got there.
    ///
    /// Not "is there a network": nothing here asks the system that, because the only
    /// question that matters is whether *this server* answered, and a phone with five
    /// bars and a server that is down is in exactly the state a phone in a basement is.
    private var reachedIt = true

    init(remote: any Backend & Destination, cache: Cache = .shared) {
        self.remote = remote
        self.cache = cache
    }

    // MARK: - What a screen can ask about the backend itself

    var reachable: Bool { reachedIt }

    var pending: Int { cache.outbox.waiting }

    // MARK: - Reading

    func lists() async throws -> Listing<List> {
        do {
            let answer = try await remote.lists()
            cache.remember(lists: answer.items)
            reachedIt = true
            // The far end answered, so anything waiting for it goes now. Here rather
            // than in a model, because "the server is reachable" is something only this
            // knows and draining is the only sensible thing to do with that news.
            await drain()
            return answer
        } catch let problem as APIError {
            guard case .transport = problem else { throw problem }
            reachedIt = false
            // What was last seen, rather than nothing. A failed load is not evidence
            // that somebody has no lists -- which is the bug the cache exists for.
            let remembered = cache.lists()
            return Listing(items: remembered, total: Int64(remembered.count), truncated: false)
        }
    }

    func items(on list: List) async throws -> Listing<Item> {
        do {
            let answer = try await remote.items(on: list)
            cache.remember(items: answer.items, on: list)
            reachedIt = true
            return answer
        } catch let problem as APIError {
            guard case .transport = problem else { throw problem }
            reachedIt = false
            let remembered = cache.items(on: list)
            return Listing(items: remembered, total: Int64(remembered.count), truncated: false)
        }
    }

    func units() async throws -> [Unit] {
        do {
            let answer = try await remote.units()
            cache.remember(units: answer)
            return answer
        } catch let problem as APIError where isTransport(problem) {
            return cache.units()
        }
    }

    func tags(orderedFor list: List) async throws -> [Tag] {
        do {
            let answer = try await remote.tags(orderedFor: list)
            cache.remember(tags: answer, on: list)
            return answer
        } catch let problem as APIError where isTransport(problem) {
            return cache.tags(on: list)
        }
    }

    func tags(on item: Item, in list: List) async throws -> [Tag] {
        try await remote.tags(on: item, in: list)
    }

    func suggestions(matching typed: String, on list: List) async throws -> [String] {
        try await remote.suggestions(matching: typed, on: list)
    }

    func history(on list: List) async throws -> [RememberedEntry] {
        let answer = try await remote.history(on: list)
        cache.adopt(history: answer, on: list)
        return answer
    }

    // MARK: - Lists

    func createList(named name: String) async throws -> List {
        do {
            return try await remote.createList(named: name)
        } catch let problem as APIError where isTransport(problem) {
            // Written down here and queued for the server. A list made with no signal
            // is a list, and the queue is what carries it when the signal comes back.
            reachedIt = false
            let made = cache.makeListHere(named: name, ownedBy: 0)
            cache.outbox.makeList(made)
            return made
        }
    }

    func rename(_ list: List, to name: String) async throws {
        try await remote.rename(list, to: name)
    }

    func delete(_ list: List) async throws {
        try await remote.delete(list)
    }

    // MARK: - What is on one

    func add(_ line: String, to list: List) async throws {
        try await remote.add(line, to: list)
    }

    func setDone(_ item: Item, on list: List, done: Bool) async throws {
        try await remote.setDone(item, on: list, done: done)
    }

    func setDone(itemID: Int64, listID: Int64, done: Bool) async throws {
        try await remote.setDone(itemID: itemID, listID: listID, done: done)
    }

    func update(
        _ item: Item,
        on list: List,
        name: String,
        amount: Double,
        unitID: Int64?
    ) async throws {
        try await remote.update(item, on: list, name: name, amount: amount, unitID: unitID)
    }

    func attach(_ tag: Tag, to item: Item, on list: List) async throws {
        try await remote.attach(tag, to: item, on: list)
    }

    func detach(_ tag: Tag, from item: Item, on list: List) async throws {
        try await remote.detach(tag, from: item, on: list)
    }

    func clearDone(on list: List) async throws {
        try await remote.clearDone(on: list)
    }

    func delete(_ item: Item, on list: List) async throws {
        try await remote.delete(item, on: list)
    }

    // MARK: - The categories

    func setTagOrder(_ tags: [Tag], on list: List) async throws {
        try await remote.setTagOrder(tags, on: list)
    }

    func createTag(named name: String, emoji: String?) async throws -> Tag {
        try await remote.createTag(named: name, emoji: emoji)
    }

    func updateTag(_ tag: Tag, named name: String, emoji: String?) async throws -> Tag {
        try await remote.updateTag(tag, named: name, emoji: emoji)
    }

    func deleteTag(_ tag: Tag) async throws {
        try await remote.deleteTag(tag)
    }

    // MARK: - Somebody else changed something

    /// Two sources, one stream.
    ///
    /// A server tells this backend when somebody else changed something. The *cache*
    /// tells it when something on this device did -- a tick from the watch, a drain
    /// adopting the server's ids, a sheet writing through. Both mean the same thing to a
    /// screen, which is "read again", so both arrive the same way.
    ///
    /// Merging them here rather than leaving the screen to watch the cache itself is the
    /// point of this type: the cache is not the screen's to know about. It also fixes
    /// the shape by construction -- a screen cannot watch one and forget the other,
    /// which is how three of four screens ended up not watching at all.
    func listChanges() async throws -> AsyncThrowingStream<Void, Error> {
        let remote = self.remote
        let cache = self.cache

        return AsyncThrowingStream { continuation in
            let fromServer = Task {
                // Not fatal on its own: with the server unreachable the cache half still
                // works, and the loop that consumes this reconnects.
                if let stream = try? await remote.listChanges() {
                    for try await _ in stream { continuation.yield() }
                }
            }
            let fromHere = Task {
                guard let stream = cache.observeLists() else { return }
                for try await _ in stream { continuation.yield() }
            }

            continuation.onTermination = { _ in
                fromServer.cancel()
                fromHere.cancel()
            }
        }
    }

    func changes(on list: List) async throws -> AsyncThrowingStream<Void, Error> {
        try await remote.changes(on: list)
    }

    // MARK: - The queue

    /// Empties the outbox, and adopts the ids the server minted for anything made here.
    ///
    /// Not public: a caller that has to remember to drain is a caller that will forget,
    /// which is what the two-second timer and the per-screen `sendQueued` were working
    /// around. This runs when the server has just proved it is reachable, which is the
    /// only moment draining can succeed.
    private var draining = false

    private func drain() async {
        // Anything made while there was no server at all, handed over now that there is
        // one. Cheap when there is nothing to hand over, which is the ordinary case.
        cache.handOverIfNeeded()

        guard !draining, cache.outbox.waiting > 0 else { return }
        draining = true
        let drained = await cache.outbox.drain(through: remote)
        draining = false

        // Lists made here have just been given the server's own ids. Without this the
        // same list appears twice, once under each numbering.
        for adopted in drained.adopted {
            if let local = cache.lists().first(where: { $0.uuid == adopted.uuid }) {
                cache.adopt(local, as: adopted.real)
            }
        }
    }

    private nonisolated func isTransport(_ problem: APIError) -> Bool {
        if case .transport = problem { return true }
        return false
    }
}

extension CachingBackend: Backend {}

#endif
