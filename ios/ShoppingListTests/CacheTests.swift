import Foundation
import Testing

@testable import ShoppingList

/// What the app remembers when the server cannot be reached.
///
/// Every test runs against an in-memory database, so the cache on the machine running
/// them is neither read nor overwritten.
struct CacheTests {

    private func list(id: Int64 = 1, name: String = "Dairy") -> List {
        List(id: id, uuid: "1111-\(id)", name: name, ownerID: 9, role: .editor)
    }

    private func item(id: Int64, name: String, done: Bool = false) -> Item {
        Item(
            id: id,
            uuid: "2222-\(id)",
            name: name,
            amount: 2,
            unitID: 3,
            doneAt: done ? Date(timeIntervalSince1970: 1_700_000_000) : nil,
            tagIDs: [4, 5]
        )
    }

    @Test func remembersListsInTheOrderTheyArrived() {
        let cache = Cache.inMemory()
        let sent = [list(id: 3, name: "Bakery"), list(id: 1, name: "Dairy")]

        cache.remember(lists: sent)

        // Not sorted by id: the server decides the order, and a cached screen has to
        // come back looking like the one that was cached.
        #expect(cache.lists().map(\.name) == ["Bakery", "Dairy"])
        #expect(cache.lists().map(\.uuid) == sent.map(\.uuid))
        #expect(cache.lists().first?.role == .editor)
    }

    @Test func rememberingReplacesRatherThanAccumulates() {
        let cache = Cache.inMemory()

        cache.remember(lists: [list(id: 1), list(id: 2, name: "Bakery")])
        cache.remember(lists: [list(id: 1)])

        // A list somebody deleted must not survive in the cache as a row that can
        // still be opened.
        #expect(cache.lists().count == 1)
    }

    @Test func remembersWhatWasOnAList() {
        let cache = Cache.inMemory()
        let dairy = list()
        let sent = [item(id: 1, name: "Milk"), item(id: 2, name: "Butter", done: true)]

        cache.remember(items: sent, on: dairy)
        let back = cache.items(on: dairy)

        #expect(back.map(\.name) == ["Milk", "Butter"])
        #expect(back.map(\.uuid) == sent.map(\.uuid))
        #expect(back[0].isDone == false)
        #expect(back[1].isDone == true)
        // The tag ids survive the trip through one text column, which is the only
        // part of this shape that is not simply a value.
        #expect(back[0].tagIDs == [4, 5])
        #expect(back[0].amount == 2)
        #expect(back[0].unitID == 3)
    }

    @Test func itemsAreKeptPerList() {
        let cache = Cache.inMemory()
        let dairy = list(id: 1)
        let bakery = list(id: 2, name: "Bakery")

        cache.remember(items: [item(id: 1, name: "Milk")], on: dairy)
        cache.remember(items: [item(id: 2, name: "Bread")], on: bakery)

        #expect(cache.items(on: dairy).map(\.name) == ["Milk"])
        #expect(cache.items(on: bakery).map(\.name) == ["Bread"])
    }

    /// Units and tags too: a list read with no signal should still be measured and
    /// filed rather than a column of bare names.
    @Test func remembersUnitsAndTags() {
        let cache = Cache.inMemory()
        let dairy = list()

        cache.remember(units: [Unit(id: 1, name: "kg"), Unit(id: 2, name: "unit")])
        cache.remember(
            tags: [
                Tag(id: 7, name: "dairy", emoji: "🥛", sortOrder: 3),
                Tag(id: 8, name: "bakery", emoji: nil, sortOrder: 9),
            ],
            on: dairy
        )

        #expect(cache.units().map(\.name) == ["kg", "unit"])
        #expect(cache.tags(on: dairy).map(\.name) == ["dairy", "bakery"])
        #expect(cache.tags(on: dairy).first?.emoji == "🥛")
        // Renumbered from the stored order rather than kept: the position is what the
        // person's own ordering resolved to, and a stored value would read as a tie.
        #expect(cache.tags(on: dairy).map(\.sortOrder) == [0, 1])
    }

    @Test func signingOutLeavesNothingBehind() {
        let cache = Cache.inMemory()
        let dairy = list()
        cache.remember(lists: [dairy])
        cache.remember(items: [item(id: 1, name: "Milk")], on: dairy)
        cache.remember(units: [Unit(id: 1, name: "kg")])

        cache.forgetEverything()

        #expect(cache.lists().isEmpty)
        #expect(cache.items(on: dairy).isEmpty)
        #expect(cache.units().isEmpty)
    }

    /// Nothing cached is not the same as nothing on the list, and the difference is
    /// the whole point — but at this layer it is simply an empty answer with no
    /// throw, which is what lets the views decide what to say.
    @Test func anEmptyCacheAnswersEmptyRatherThanFailing() {
        let cache = Cache.inMemory()

        #expect(cache.lists().isEmpty)
        #expect(cache.items(on: list()).isEmpty)
        #expect(cache.tags(on: list()).isEmpty)
    }
}
