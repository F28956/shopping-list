import Foundation
import Testing

@testable import ShoppingList

/// The device answering for itself.
///
/// Every one of these goes through `any Backend` rather than through `LocalBackend`,
/// and that is the assertion being made: a screen holding the protocol cannot tell
/// which conformer it has. If these had to reach for the concrete type to work, the
/// substitution the whole exercise depends on would not be real.
///
/// Against a database in a temporary directory, so the one on the machine running these
/// is neither read nor written.
struct LocalBackendTests {

    /// A backend over a database of its own, removed when the test ends.
    ///
    /// Named `opened` rather than `backend`: the local `backend` in each test shadows
    /// it, and the shadowing is invisible until `#require` expands and reports that a
    /// value is not a function.
    private func opened() -> (any Backend, () -> Void)? {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("local-\(UUID().uuidString).sqlite")
        guard let made = LocalBackend(at: path) else { return nil }
        return (made, { try? FileManager.default.removeItem(at: path) })
    }

    @Test("a list made on the device comes back as the device's own")
    func aListIsMadeAndRead() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }

        let made = try await backend.createList(named: "Household")
        let seen = try await backend.lists()

        #expect(seen.items.map(\.name) == ["Household"])
        #expect(seen.items.first?.id == made.id)
        // Owner, not viewer. The bare row `domain` returns has no role at all -- the
        // API adds one, so the device adds one, or every list it owns would come back
        // read-only and the app would hide renaming and deleting on all of them.
        #expect(seen.items.first?.role == .owner, "the device does not own its own list")
        #expect(seen.items.first?.uuid.isEmpty == false, "no uuid to name it by")
    }

    /// The line is read by the *server's* reader, which is the whole reason for this.
    ///
    /// `2 kg apples` has to become the same row here as it does in a browser. It used
    /// to be read twice -- once on the phone to draw the row, once on the server to
    /// store it -- and the two could disagree, which is how `pint milk` became one unit
    /// of `pint milk` on a phone and one pint of milk everywhere else.
    @Test("a typed line is read the way the server reads it")
    func aTypedLineIsResolved() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")

        try await backend.add("2 kg apples", to: list)

        let items = try await backend.items(on: list)
        let apples = try #require(items.items.first)
        #expect(apples.name == "Apples", "the name was echoed rather than read")
        #expect(apples.amount == 2)
        #expect(apples.unitID != nil, "kg was not recognised as a unit")
    }

    /// The one that was wrong on the phone for a whole afternoon.
    @Test("a bare unit is read as one of it")
    func aBareUnitIsUnderstood() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")

        try await backend.add("pint milk", to: list)

        let milk = try #require(try await backend.items(on: list).items.first)
        #expect(milk.name == "Milk")
        #expect(milk.amount == 1, "a pint of milk was read as one unit of `pint milk`")
        #expect(milk.unitID != nil, "pint was not read as a unit")
    }

    @Test("what is on a list can be ticked off, corrected and cleared")
    func aListIsShopped() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")
        try await backend.add("milk", to: list)
        var milk = try #require(try await backend.items(on: list).items.first)

        try await backend.update(milk, on: list, name: "Oat milk", amount: 2, unitID: nil)
        milk = try #require(try await backend.items(on: list).items.first)
        #expect(milk.name == "Oat milk")
        #expect(milk.amount == 2)

        try await backend.setDone(milk, on: list, done: true)
        milk = try #require(try await backend.items(on: list).items.first)
        #expect(milk.isDone, "ticking off did not stick")

        try await backend.clearDone(on: list)
        #expect(try await backend.items(on: list).items.isEmpty)
    }

    /// Categories are the household's vocabulary, and on a device the person *is* the
    /// household -- so they may change them. On a server that is the owner's alone, and
    /// the same rule decides both: the device's person owns the device.
    @Test("the categories are this person's to change")
    func categoriesAreEditable() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")

        let shipped = try await backend.tags(orderedFor: list)
        #expect(!shipped.isEmpty, "a device with no categories has no way to group anything")

        // Lowercased, and asserted rather than worked around: `tag::Name` normalises
        // -- trimmed and folded -- so that `Dairy`, `dairy ` and `DAIRY` cannot become
        // three categories. The twenty-one shipped ones are stored that way too, which
        // is what a device already holds.
        //
        // It is a visible change all the same. The client's own `Cache.addTag` keeps
        // what was typed, so somebody who types `Fishmonger` today sees it back with
        // the capital and would not here. Whether the screen should capitalise for
        // display is a question for the screen, and `parsing::capitalise` already
        // exists to answer it.
        let made = try await backend.createTag(named: "Fishmonger", emoji: "🐟")
        #expect(made.name == "fishmonger")

        let renamed = try await backend.updateTag(made, named: "Fish", emoji: nil)
        #expect(renamed.name == "fish")

        try await backend.deleteTag(renamed)
        let after = try await backend.tags(orderedFor: list)
        #expect(!after.contains { $0.id == made.id }, "a deleted category came back")
    }

    /// Filing a row, which is what decides the order a shop is walked in.
    @Test("a row can be filed and unfiled")
    func aRowIsFiled() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")
        try await backend.add("milk", to: list)
        let milk = try #require(try await backend.items(on: list).items.first)
        let dairy = try #require(try await backend.tags(orderedFor: list).first)

        try await backend.attach(dairy, to: milk, on: list)
        var filed = try #require(try await backend.items(on: list).items.first)
        #expect(filed.tagIDs == [dairy.id], "filing did not reach the row")

        try await backend.detach(dairy, from: milk, on: list)
        filed = try #require(try await backend.items(on: list).items.first)
        #expect(filed.tagIDs.isEmpty)
    }

    /// The order is per list, the vocabulary is global -- the shape the client's own
    /// cache was rebuilt into earlier today, now decided in one place instead of two.
    @Test("a list keeps its own order over the shared categories")
    func aListKeepsItsOwnOrder() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")
        let asShipped = try await backend.tags(orderedFor: list)

        try await backend.setTagOrder(asShipped.reversed(), on: list)

        let after = try await backend.tags(orderedFor: list)
        #expect(after.map(\.id) == asShipped.reversed().map(\.id), "the walk was not reordered")
    }

    /// What has been bought before, which is what a part-typed line is matched against.
    @Test("what was bought is remembered and offered back")
    func whatWasBoughtIsRemembered() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let list = try await backend.createList(named: "Household")
        try await backend.add("milk", to: list)

        #expect(try await backend.history(on: list).isEmpty == false, "nothing was remembered")
        let offered = try await backend.suggestions(matching: "mil", on: list)
        #expect(offered.contains("Milk"), "milk was not offered for `mil`: \(offered)")
    }

    /// Reopening is what every launch does.
    @Test("a device comes back to what it had")
    func itSurvivesBeingReopened() async throws {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("local-\(UUID().uuidString).sqlite")
        defer { try? FileManager.default.removeItem(at: path) }

        let first: any Backend = try #require(LocalBackend(at: path))
        _ = try await first.createList(named: "Household")

        let again: any Backend = try #require(LocalBackend(at: path))
        #expect(try await again.lists().items.map(\.name) == ["Household"])
    }

    /// A refusal has to arrive as an error, not as empty rows. A screen that cannot
    /// tell "there is nothing" from "I could not find out" is the bug the cache was
    /// built for in the first place.
    @Test("asking about a list that is not there is refused, not answered emptily")
    func aMissingListIsRefused() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }
        let absent = List(id: 9_999, uuid: "nope", name: "Gone", ownerID: 1, role: .owner)

        await #expect(throws: (any Error).self) {
            _ = try await backend.items(on: absent)
        }
    }

    /// The screen being told, which is the half that cannot be checked by reading.
    ///
    /// Underneath this is `domain`'s broadcast channel -- the one the server drives SSE
    /// from -- reaching Swift through a blocking call on a thread of its own. A `Task`
    /// would not do: a blocked task holds a cooperative pool thread, and enough of them
    /// starve the app.
    @Test("a change reaches a watcher on the device")
    func aChangeReachesAWatcher() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }

        let changes = try await backend.listChanges()
        var iterator = changes.makeAsyncIterator()

        // Made after the watch starts, because the channel carries what happens next
        // rather than what has already happened -- an app returning from the background
        // is told nothing and re-reads everything, which is the right way round.
        Task { _ = try? await backend.createList(named: "Household") }

        let heard = try await withThrowingTaskGroup(of: Bool.self) { group in
            group.addTask { try await iterator.next() != nil }
            group.addTask {
                try await Task.sleep(for: .seconds(5))
                return false
            }
            let first = try await group.next()
            group.cancelAll()
            return first ?? false
        }

        #expect(heard, "nothing reached the watcher within five seconds")
    }

    /// Cancelling has to end the thread, or every screen that closes leaves one parked
    /// for the life of the app.
    @Test("a watch that is cancelled lets go")
    func aWatchLetsGo() async throws {
        let (backend, clean) = try #require(opened())
        defer { clean() }

        let watching = Task {
            let changes = try await backend.listChanges()
            for try await _ in changes { return }
        }
        // Long enough for the thread to have parked in `embedded_next_change`, which is
        // the case that matters: cancelling before it parks proves nothing.
        try await Task.sleep(for: .milliseconds(200))

        watching.cancel()

        // The backend still answers, which is the thing a leaked or deadlocked watcher
        // would take away.
        _ = try await backend.createList(named: "Household")
        #expect(try await backend.lists().items.count == 1)
    }

    // MARK: - The lists screen, driven by the device

    /// The first screen to run on this, and the point of the whole exercise.
    ///
    /// Note what is *not* asserted: no queue, no cache, no offline. The model is given
    /// a backend and nothing else, and the screen works -- which is what "standalone is
    /// not offline" means in code rather than in a comment.
    @MainActor
    @Test("the lists screen runs on the device's own backend")
    func theListsScreenRunsLocally() async throws {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("screen-\(UUID().uuidString).sqlite")
        defer { try? FileManager.default.removeItem(at: path) }
        let backend = try #require(LocalBackend(at: path))

        // A backend and nothing else: no accounts, no cache, no queue.
        let model = ListsModel(api: backend)

        await model.load()

        #expect(model.loaded)
        #expect(model.fresh, "a device that answered was treated as not having answered")
        #expect(!model.offline, "a device answering for itself reported itself unreachable")
        #expect(model.error == nil)
        #expect(model.waiting == 0, "something was queued for nobody")
        // The person using the device administers it, so the screens behind that are
        // offered rather than hidden.
        #expect(model.isOwner, "the device's person does not administer the device")

        let made = try #require(await model.makeList(named: "Household"))
        #expect(made.name == "Household")
        #expect(model.lists.map(\.name) == ["Household"])
        #expect(model.error == nil, "making a list on the device was reported as a failure")
        #expect(model.waiting == 0, "a list made on the device was queued for a server")
    }

    /// What the migration is for, and it was written after breaking it: switching a
    /// device that has been used showed an empty app with somebody's shopping still on
    /// disk. That happened on the first Mac it was tried on -- one list, three items.
    @Test("a device's old cache is brought across")
    func theOldCacheIsBroughtAcross() async throws {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("migrate-\(UUID().uuidString).sqlite")
        defer { try? FileManager.default.removeItem(at: path) }

        // As the old path left it: a list, something needed, something crossed off.
        let old = Cache.inMemory(sending: { false })
        let list = old.makeListHere(named: "Home", ownedBy: 0)
        old.remember(
            items: [
                Item(id: -2, uuid: "milk", name: "Milk", amount: 2,
                     unitID: nil, doneAt: nil, tagIDs: [1]),
                Item(id: -3, uuid: "apples", name: "Apples", amount: 1,
                     unitID: nil, doneAt: Date(timeIntervalSince1970: 1_787_908_502), tagIDs: []),
            ],
            on: list
        )

        let backend = try #require(LocalBackend(at: path))
        #expect(backend.bringAcross(old.lists(), from: old), "the migration refused")

        let carried: any Backend = backend
        let lists = try await carried.lists()
        #expect(lists.items.map(\.name) == ["Home"])

        let items = try await carried.items(on: try #require(lists.items.first))
        #expect(items.items.count == 2, "not everything came across")
        let milk = try #require(items.items.first { $0.name == "Milk" })
        #expect(milk.amount == 2, "how much was lost")
        #expect(milk.tagIDs == [1], "what it was filed under was lost")
        #expect(!milk.isDone, "something still needed arrived crossed off")
        #expect(
            items.items.first { $0.name == "Apples" }?.isDone == true,
            "something crossed off arrived still needed"
        )

        // And the memory, because every row came in through the service that records a
        // use -- so a device does not forget what it buys just because it moved house.
        let offered = try await carried.suggestions(matching: "mil", on: try #require(lists.items.first))
        #expect(offered.contains("Milk"), "autocomplete forgot what the device knew")
    }

    /// Nothing is deleted, which is what makes the switch reversible.
    @Test("the old cache is left exactly as it was")
    func theOldCacheIsNotEmptied() async throws {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("migrate-\(UUID().uuidString).sqlite")
        defer { try? FileManager.default.removeItem(at: path) }

        let old = Cache.inMemory(sending: { false })
        let list = old.makeListHere(named: "Home", ownedBy: 0)
        old.remember(
            items: [Item(id: -2, uuid: "milk", name: "Milk", amount: 1,
                         unitID: nil, doneAt: nil, tagIDs: [])],
            on: list
        )

        let backend = try #require(LocalBackend(at: path))
        _ = backend.bringAcross(old.lists(), from: old)

        #expect(old.lists().map(\.name) == ["Home"], "the fallback was emptied")
        #expect(old.items(on: list).count == 1, "the fallback lost what was on the list")
    }

    /// The gap the typed stream exposed: `domain` announces when a row is filed but
    /// **not** when a category is created, renamed, removed or reordered -- a category
    /// belongs to no list, so there is no channel for "the vocabulary moved".
    ///
    /// Without this a rename in Settings would not reach an open list on a device
    /// answering for itself: the same bug that was fixed on the cached path, arriving
    /// by a different road.
    @Test("renaming a category tells an open list")
    func aRenameReachesAnOpenList() async throws {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("nudge-\(UUID().uuidString).sqlite")
        defer { try? FileManager.default.removeItem(at: path) }
        let backend = try #require(LocalBackend(at: path))

        let list = try await backend.createList(named: "Household")
        let dairy = try #require(try await backend.tags(orderedFor: list).first)
        var nudges = try await backend.changes(on: list).makeAsyncIterator()

        Task { _ = try? await backend.updateTag(dairy, named: "cheese counter", emoji: nil) }

        let heard = try await withThrowingTaskGroup(of: Nudge?.self) { group in
            group.addTask { try await nudges.next() }
            group.addTask {
                try await Task.sleep(for: .seconds(5))
                return nil
            }
            let first = try await group.next()
            group.cancelAll()
            return first ?? nil
        }

        if case .categories = try #require(heard) {} else {
            Issue.record("a rename was reported as \(String(describing: heard))")
        }
    }
}

// MARK: - Handing a standalone device to a server

/// The journey that had no code: everything a device made while answering for itself,
/// getting to a server the day somebody chooses one.
///
/// `readyForUse` carries the old cache *into* `device.sqlite`. This is the other
/// direction, and until now it did not exist — so adopting a server showed an empty
/// account with a year of shopping still on disk, no error anywhere, and no way back
/// except giving the server up again.
struct HandingOverToAServerTests {

    private func device() -> (LocalBackend, () -> Void)? {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent("handover-\(UUID().uuidString).sqlite")
        guard let made = LocalBackend(at: path) else { return nil }
        return (made, { try? FileManager.default.removeItem(at: path) })
    }

    /// The whole journey, end to end: made on a device with no server, and afterwards
    /// sitting in the queue with everything the server needs to be told.
    @Test("what a standalone device holds ends up queued for the server")
    func everythingReachesTheQueue() async throws {
        let (backend, clean) = try #require(device())
        defer { clean() }

        let list = try await backend.createList(named: "Household")
        try await backend.add("2 kg apples", to: list)
        try await backend.add("milk", to: list)
        let rows = try await backend.items(on: list).items
        let milk = try #require(rows.first { $0.name.lowercased().contains("milk") })
        try await backend.setDone(milk, on: list, done: true)

        // A server appears. `sending` is true because one has been chosen.
        let cache = Cache.inMemory(sending: { true })
        let taken = try #require(backend.everythingHere())
        cache.takeIn(taken)
        cache.handOverIfNeeded()

        let queued = cache.outbox.all()
        #expect(
            queued.contains { $0.kind == QueuedOperation.Kind.makeList },
            "the list itself was never queued"
        )
        let added = queued.filter { $0.kind == QueuedOperation.Kind.add }
        #expect(added.count == 2, "expected both rows, queued \(added.count)")
        #expect(
            queued.contains { $0.kind == QueuedOperation.Kind.setDone },
            "what had been crossed off arrived as outstanding"
        )
    }

    /// The uuids have to survive, or the first drain makes a second copy of everything.
    ///
    /// `MakeList` and `Add` are both idempotent by uuid on the server. Minting new ones
    /// here — which is what `makeListHere` does, and what the obvious implementation
    /// would have reused — would mean the device and the server disagreed about the
    /// name of every row from the moment they met.
    @Test("the names the server will be told are the device's own")
    func uuidsSurviveTheCrossing() async throws {
        let (backend, clean) = try #require(device())
        defer { clean() }

        let list = try await backend.createList(named: "Household")
        try await backend.add("apples", to: list)
        let apples = try #require(try await backend.items(on: list).items.first)

        let cache = Cache.inMemory(sending: { true })
        cache.takeIn(try #require(backend.everythingHere()))

        let carried = try #require(cache.lists().first)
        #expect(carried.uuid == list.uuid, "the list was renamed on the way")
        #expect(Cache.isLocal(carried), "it arrived as something the server has heard of")
        #expect(
            cache.items(on: carried).map(\.uuid) == [apples.uuid],
            "the row was renamed on the way"
        )
    }

    /// Two lists, because the ids are minted here and the obvious loop gives the second
    /// list's rows the ids the first list's already have — which is a primary key, and
    /// which is the bug this codebase has now hit three times.
    @Test("a second list does not collide with the first")
    func twoListsBothArrive() async throws {
        let (backend, clean) = try #require(device())
        defer { clean() }

        let home = try await backend.createList(named: "Home")
        let boat = try await backend.createList(named: "Boat")
        try await backend.add("apples", to: home)
        try await backend.add("rope", to: boat)

        let cache = Cache.inMemory(sending: { true })
        cache.takeIn(try #require(backend.everythingHere()))

        #expect(cache.lists().count == 2, "one of the two lists was lost")
        for list in cache.lists() {
            #expect(
                cache.items(on: list).count == 1,
                "\(list.name) came out with \(cache.items(on: list).count) rows"
            )
        }
    }

    /// Running it twice must not make a second copy — a device that adopts a server,
    /// gives it up and adopts another has been through here more than once.
    @Test("handing over twice does not duplicate anything")
    func theCrossingIsIdempotent() async throws {
        let (backend, clean) = try #require(device())
        defer { clean() }

        let list = try await backend.createList(named: "Household")
        try await backend.add("apples", to: list)

        let cache = Cache.inMemory(sending: { true })
        let taken = try #require(backend.everythingHere())
        cache.takeIn(taken)
        cache.takeIn(taken)

        #expect(cache.lists().count == 1, "the same list arrived twice")
        #expect(cache.items(on: try #require(cache.lists().first)).count == 1)
    }

    /// And nothing is taken away. This is what makes adopting a server reversible
    /// before anybody has proved the server works.
    @Test("the device keeps everything it handed over")
    func nothingIsDeleted() async throws {
        let (backend, clean) = try #require(device())
        defer { clean() }

        let list = try await backend.createList(named: "Household")
        try await backend.add("apples", to: list)

        let cache = Cache.inMemory(sending: { true })
        cache.takeIn(try #require(backend.everythingHere()))

        #expect(try await backend.lists().items.count == 1, "the device lost its list")
        #expect(
            try await backend.items(on: list).items.count == 1,
            "the device lost its shopping"
        )
    }
}
