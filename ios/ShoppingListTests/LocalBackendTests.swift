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
}
