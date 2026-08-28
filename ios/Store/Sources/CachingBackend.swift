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
            let shown = laidOver(answer.items, on: list)
            return Listing(items: shown, total: answer.total, truncated: answer.truncated)
        } catch let problem as APIError {
            guard case .transport = problem else { throw problem }
            reachedIt = false
            let remembered = laidOver(cache.items(on: list), on: list)
            return Listing(items: remembered, total: Int64(remembered.count), truncated: false)
        }
    }

    /// The answer with this device's unsent changes laid back over it.
    ///
    /// Without this a successful read visibly undoes work that is still queued: the
    /// server has not been told, so it answers with the old state, and the rows flick
    /// back for as long as the queue is stuck.
    ///
    /// Rows this device created and has not sent are not in the answer at all, so they
    /// are carried from the cache -- which has them, because `add` writes them down.
    /// **Only rows it created.** Any queued operation used to qualify, which meant a
    /// tick queued against a row somebody else had deleted put that row back as a ghost:
    /// present here, gone everywhere else, impossible to be rid of.
    private func laidOver(_ answer: [Item], on list: List) -> [Item] {
        let queued = cache.outbox.forList(list)
        guard !queued.isEmpty else { return answer }

        let known = Set(answer.map(\.uuid))
        let made = Set(queued.filter { $0.kind == QueuedOperation.Kind.add }.map(\.itemUUID))
        var rows = answer + cache.items(on: list).filter {
            !known.contains($0.uuid) && made.contains($0.uuid)
        }

        for operation in queued {
            switch operation.kind {
            case QueuedOperation.Kind.setDone:
                rows = rows.map {
                    $0.uuid == operation.itemUUID ? $0.withDone(operation.done) : $0
                }
            case QueuedOperation.Kind.delete:
                rows = rows.filter { $0.uuid != operation.itemUUID }
            default:
                break
            }
        }
        return rows
    }

    func units() async throws -> [Unit] {
        do {
            let answer = try await remote.units()
            cache.remember(units: answer)
            return answer
        } catch let problem as APIError where isTransport(problem) {
            // The cache, then what shipped with the app. A first run with no signal
            // would otherwise have no units at all, and a row with no unit prints no
            // measure -- which reads as a row that has lost one.
            let remembered = cache.units()
            return remembered.isEmpty ? Reference.units : remembered
        }
    }

    func tags(orderedFor list: List) async throws -> [Tag] {
        do {
            let answer = try await remote.tags(orderedFor: list)
            cache.remember(tags: answer, on: list)
            return answer
        } catch let problem as APIError where isTransport(problem) {
            let remembered = cache.tags(on: list)
            return remembered.isEmpty ? Reference.tags : remembered
        }
    }

    func tags(on item: Item, in list: List) async throws -> [Tag] {
        try await remote.tags(on: item, in: list)
    }

    /// What to offer for a part-typed line.
    ///
    /// The device's own memory rather than a round trip, and not only as a fallback:
    /// autocomplete that waits for a network answers after the next letter is typed.
    /// The ranking is `parsing::suggest`, which the server runs too -- so the same
    /// letters offer the same things in the same order either way.
    func suggestions(matching typed: String, on list: List) async throws -> [String] {
        QuickAdd.suggest(typed, from: cache.history(on: list))
    }

    func history(on list: List) async throws -> [RememberedEntry] {
        let answer = try await remote.history(on: list)
        cache.adopt(history: answer, on: list)
        return answer
    }

    /// Records what somebody corrected a row *to*.
    ///
    /// A better memory than what they first typed, which is why the server records
    /// history on an edit as well as on an add. The count does not rise: editing one row
    /// twice is one intention, not two.
    private func remember(_ item: Item, on list: List) {
        cache.remember(item, on: list, isNew: false)
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

    /// Reads what somebody typed, and queues what it turned out to be.
    ///
    /// The reading is `QuickAdd.resolve`, which is `parsing::add` compiled in and shared
    /// with the server -- which unit a bare name lands in, whether `Milk` is the `milk`
    /// already on the list, whether a crossed-off row comes back. It happens here rather
    /// than on a screen because it exists for one reason: a device with no signal has to
    /// decide for itself, and deciding for itself is this type's whole job.
    ///
    /// `LocalBackend` does none of this. It hands the line to `domain`, which reads it
    /// with the same rules on the other side of the boundary -- so a screen calls
    /// `add(line)` and neither knows nor cares which happened.
    func add(_ line: String, to list: List) async throws {
        let decision = QuickAdd.resolve(
            line,
            units: cache.units(),
            rows: cache.items(on: list),
            // The whole memory. Picking one entry by the typed line finds nothing for
            // anything carrying a quantity -- see `QuickAdd.resolve`.
            history: cache.history(on: list)
        )

        switch decision {
        case .existing(let uuid, let putBack):
            guard let alike = cache.items(on: list).first(where: { $0.uuid == uuid }) else {
                return
            }
            // Queued under a **fresh** uuid: the server runs this same rule when it
            // hears the line, and handing it the row's own uuid takes the early return
            // in `create` and skips the putting back.
            //
            // Its own name and amount, not the typed line -- this is the row the shared
            // rule chose, and saying so leaves the server nothing to choose.
            cache.outbox.add(
                uuid: UUID().uuidString.lowercased(),
                localID: alike.id,
                name: alike.name,
                amount: alike.amount,
                unitID: alike.unitID,
                on: list
            )
            cache.remember(alike, on: list, isNew: true)
            if putBack {
                // Queued as well as shown, and that is not belt and braces: the overlay
                // replays queued ticks in order, so the `setDone(true)` that crossed
                // this row off is still in there. Without a later `setDone(false)` the
                // next read puts the tick straight back and the row somebody just
                // re-added appears already done.
                cache.outbox.setDone(alike, on: list, done: false)
                remembering(on: list) { rows in
                    rows.map { $0.uuid == uuid ? $0.withDone(false) : $0 }
                }
            }

        case .new(let row):
            // A **negative id** that never leaves the device: a placeholder so a screen
            // has something to key on. The uuid is what the operation actually names,
            // and what the server's row comes back under.
            let made = Item(
                id: -Int64(Date().timeIntervalSince1970 * 1000),
                uuid: UUID().uuidString.lowercased(),
                name: row.name,
                amount: row.amount,
                unitID: row.unitID,
                doneAt: nil,
                // Where the history says it belongs, so a re-added item files itself.
                tagIDs: row.tagIDs
            )
            cache.outbox.add(
                uuid: made.uuid,
                localID: made.id,
                name: made.name,
                amount: made.amount,
                unitID: made.unitID,
                on: list
            )
            // Said out loud, because the add itself has no field for tags and the
            // server's filing step only runs when it is given a line. Behind the add in
            // an ordered queue, so the row exists by the time these land.
            for tagID in made.tagIDs {
                cache.outbox.tag(made, on: list, tagID: tagID, attached: true)
            }
            cache.remember(made, on: list, isNew: true)
            remembering(on: list) { $0 + [made] }
        }

        await drain()
    }

    /// Queued, then sent -- rather than sent, and queued if that fails.
    ///
    /// The order is the promise. A change somebody has already watched happen must
    /// survive the app being killed a second later, so it is written down before
    /// anything is attempted. If the send fails it stays in the queue and the next
    /// drain carries it; if the app dies, it is still there.
    ///
    /// This is how `ItemsModel` has always worked. It lives here now because *why* it
    /// works this way -- a far end that can fail -- is this type's business.
    func setDone(_ item: Item, on list: List, done: Bool) async throws {
        cache.outbox.setDone(item, on: list, done: done)
        remembering(on: list) { rows in
            rows.map { $0.uuid == item.uuid ? $0.withDone(done) : $0 }
        }
        await drain()
    }

    func setDone(itemID: Int64, listID: Int64, done: Bool) async throws {
        try await remote.setDone(itemID: itemID, listID: listID, done: done)
    }

    /// What is queued against one list, by row.
    func unsent(on list: List) async -> Set<String> {
        Set(cache.outbox.forList(list).map(\.itemUUID))
    }

    /// Sends what is waiting. See ``SyncReport``.
    @discardableResult
    func sync() async -> SyncReport {
        await drain()
    }

    func update(
        _ item: Item,
        on list: List,
        name: String,
        amount: Double,
        unitID: Int64?
    ) async throws {
        // The row as it was travels with it -- `seen` is how the server tells a rename
        // from two people correcting the same row, and the outbox is what carries it.
        cache.outbox.update(item, on: list, name: name, amount: amount, unitID: unitID)
        let corrected = Item(
            id: item.id,
            uuid: item.uuid,
            name: name,
            amount: amount,
            unitID: unitID,
            doneAt: item.doneAt,
            tagIDs: item.tagIDs
        )
        remembering(on: list) { rows in
            rows.map { $0.uuid == item.uuid ? corrected : $0 }
        }
        remember(corrected, on: list)
        await drain()
    }

    func attach(_ tag: Tag, to item: Item, on list: List) async throws {
        cache.outbox.tag(item, on: list, tagID: tag.id, attached: true)
        await drain()
    }

    func detach(_ tag: Tag, from item: Item, on list: List) async throws {
        cache.outbox.tag(item, on: list, tagID: tag.id, attached: false)
        await drain()
    }

    /// Everything crossed off, by name.
    ///
    /// The rows are read here rather than passed in: the queue records *which* rows were
    /// swept, so that a row somebody else crossed off in the meantime is not swept by
    /// this device's replay of an older intention.
    func clearDone(on list: List) async throws {
        let swept = cache.items(on: list).filter(\.isDone)
        guard !swept.isEmpty else { return }
        cache.outbox.clearDone(swept, on: list)
        remembering(on: list) { rows in rows.filter { !$0.isDone } }
        await drain()
    }

    func delete(_ item: Item, on list: List) async throws {
        cache.outbox.delete(item, on: list)
        remembering(on: list) { rows in rows.filter { $0.uuid != item.uuid } }
        await drain()
    }

    /// Applies a change to what is written down, as well as queueing it.
    ///
    /// Both halves matter and they are different promises. The queue says the server
    /// will hear about it; this says the *device* will still know about it after being
    /// killed on the way out of the shop. `ItemsModel.show` used to do this, back when a
    /// screen owned the cache -- and moving the queueing here without it would have
    /// meant a tick that survived until the next launch and no further.
    private func remembering(on list: List, _ change: ([Item]) -> [Item]) {
        cache.remember(items: change(cache.items(on: list)), on: list)
    }

    // MARK: - The categories

    /// Written down, then queued rather than sent.
    ///
    /// It used to go straight at the server and put "Something went wrong" on screen
    /// when it could not get there -- on a device that had deliberately not got one.
    func setTagOrder(_ tags: [Tag], on list: List) async throws {
        cache.remember(tags: tags, on: list)
        cache.outbox.setTagOrder(tags, on: list)
        await drain()
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

    /// Two sources again, for the same reason as ``listChanges()``: a change made on
    /// this device reaches the cache, a change made elsewhere reaches the server, and a
    /// screen wants to hear about both without knowing there are two.
    /// Two sources again, and now they say what they are about.
    ///
    /// The server's events are all `.rows` -- `domain` does not announce when a category
    /// changes, so a rename made in a browser genuinely does not arrive. The local half
    /// is what covers the case that matters here: a category edited in this app's own
    /// Settings writes to the cache, and comparing the vocabulary against the last one
    /// seen says which kind of nudge it was.
    ///
    /// Comparing rather than watching two tables: `Cache.observe(list:)` fetches items,
    /// units and categories together for a reason -- a screen given new rows beside old
    /// categories has a moment where a row is filed under an aisle that no longer
    /// exists. Splitting it into two observations would reintroduce exactly that.
    func changes(on list: List) async throws -> AsyncThrowingStream<Nudge, Error> {
        let remote = self.remote
        let cache = self.cache

        return AsyncThrowingStream { continuation in
            let fromServer = Task {
                if let stream = try? await remote.changes(on: list) {
                    for try await nudge in stream { continuation.yield(nudge) }
                }
            }
            let fromHere = Task {
                guard let stream = cache.observe(list: list) else { return }
                var seen: (tags: [Tag], units: [Unit])?

                for try await contents in stream {
                    defer { seen = (contents.tags, contents.units) }

                    // The first value cannot be compared with anything, and guessing
                    // wrong in either direction is a bug: called `.rows`, a category
                    // change that landed before the watch started is never fetched;
                    // called `.categories` every time, a screen pays a reference read
                    // per tick. So the first is `.categories` and the rest are compared.
                    //
                    // That costs one reference read when a list opens, which the screen
                    // does anyway. It is not a race that can be closed by reading the
                    // vocabulary first: the change can land on either side of that read.
                    guard let last = seen else {
                        continuation.yield(.categories)
                        continue
                    }
                    let moved = contents.tags != last.tags || contents.units != last.units
                    continuation.yield(moved ? .categories : .rows)
                }
            }
            continuation.onTermination = { _ in
                fromServer.cancel()
                fromHere.cancel()
            }
        }
    }

    // MARK: - The queue

    /// Empties the outbox, and adopts the ids the server minted for anything made here.
    ///
    /// Not public: a caller that has to remember to drain is a caller that will forget,
    /// which is what the two-second timer and the per-screen `sendQueued` were working
    /// around. This runs when the server has just proved it is reachable, which is the
    /// only moment draining can succeed.
    private var draining = false

    @discardableResult
    private func drain() async -> SyncReport {
        // Anything made while there was no server at all, handed over now that there is
        // one. Cheap when there is nothing to hand over, which is the ordinary case.
        cache.handOverIfNeeded()

        guard !draining, cache.outbox.waiting > 0 else {
            return SyncReport(waiting: cache.outbox.waiting)
        }
        draining = true
        let drained = await cache.outbox.drain(through: remote)
        draining = false

        // A drain that sent nothing while something was queued is the other way to
        // learn there is no connection, and often the first: it does not wait for a
        // read to fail.
        if drained.sent > 0 {
            reachedIt = true
        } else if drained.waiting > 0 && !drained.refused {
            reachedIt = false
        }

        // Lists made here have just been given the server's own ids. Without this the
        // same list appears twice, once under each numbering.
        for adopted in drained.adopted {
            if let local = cache.lists().first(where: { $0.uuid == adopted.uuid }) {
                cache.adopt(local, as: adopted.real)
            }
        }

        return SyncReport(
            sent: drained.sent,
            waiting: drained.waiting,
            refused: drained.refused,
            lost: drained.lost
        )
    }

    private nonisolated func isTransport(_ problem: APIError) -> Bool {
        if case .transport = problem { return true }
        return false
    }
}

extension CachingBackend: Backend {}

#endif
