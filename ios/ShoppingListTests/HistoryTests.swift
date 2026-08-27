import Testing
import Foundation
@testable import ShoppingList

/// What this device remembers about what gets bought.
///
/// The server keeps one of these per person per list; this is the device's own, for
/// when there is no server to ask. In memory throughout, so the history on the machine
/// running these is neither read nor written.
struct HistoryTests {
    private let list = List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)

    private func item(
        _ name: String,
        unit: Int64? = nil,
        tags: [Int64] = []
    ) -> Item {
        Item(
            id: 1,
            uuid: "item-\(name)",
            name: name,
            amount: 1,
            unitID: unit,
            doneAt: nil,
            tagIDs: tags
        )
    }

    @Test("an item that has been bought is remembered with what it was")
    func remembersWhatItWas() {
        let cache = Cache.inMemory()
        cache.remember(item("Milk", unit: 4, tags: [7]), on: list, isNew: true)

        let found = cache.remembered("Milk", on: list)
        #expect(found?.unitID == 4)
        #expect(found?.tagIDs == [7])
        #expect(found?.uses == 1)
    }

    @Test("the same name in another case is the same habit")
    func caseDoesNotMakeASecondHabit() {
        let cache = Cache.inMemory()
        cache.remember(item("Milk", unit: 4), on: list, isNew: true)
        cache.remember(item("milk", unit: 4), on: list, isNew: true)

        #expect(cache.history(on: list).count == 1)
        #expect(cache.remembered("MILK", on: list)?.uses == 2)
    }

    @Test("an add that names no aisle does not erase the one already learned")
    func anAddDoesNotForgetTheAisle() {
        // The case this exists for: somebody removes milk and types `milk` again. The
        // new line says nothing about dairy, and must not be read as saying "not dairy".
        let cache = Cache.inMemory()
        cache.remember(item("Milk", unit: 4, tags: [7]), on: list, isNew: true)
        cache.remember(item("Milk"), on: list, isNew: true)

        #expect(cache.remembered("Milk", on: list)?.tagIDs == [7])
        #expect(cache.remembered("Milk", on: list)?.unitID == 4, "the unit went too")
    }

    @Test("an edit that clears the aisles is obeyed")
    func anEditCanUnfileSomething() {
        // The other half of the rule above. An edit is somebody looking at the aisles
        // and saying "not there", which is a different sentence from not mentioning it.
        let cache = Cache.inMemory()
        cache.remember(item("Milk", tags: [7]), on: list, isNew: true)
        cache.remember(item("Milk", tags: []), on: list, isNew: false)

        #expect(cache.remembered("Milk", on: list)?.tagIDs == [])
    }

    @Test("editing does not count as buying it again")
    func editingDoesNotRaiseTheCount() {
        let cache = Cache.inMemory()
        cache.remember(item("Milk"), on: list, isNew: true)
        cache.remember(item("Milk"), on: list, isNew: false)
        cache.remember(item("Milk"), on: list, isNew: false)

        #expect(cache.remembered("Milk", on: list)?.uses == 1)
    }

    @Test("two lists are two habits")
    func aNameIsRememberedPerList() {
        let cache = Cache.inMemory()
        let office = List(id: 2, uuid: "list-2", name: "Office", ownerID: 9, role: .editor)
        cache.remember(item("Milk", unit: 4), on: list, isNew: true)
        cache.remember(item("Milk", unit: 9), on: office, isNew: true)

        #expect(cache.remembered("Milk", on: list)?.unitID == 4)
        #expect(cache.remembered("Milk", on: office)?.unitID == 9)
    }

    @Test("forgetting one is the way back from a typo")
    func forgetting() {
        let cache = Cache.inMemory()
        cache.remember(item("Mikl"), on: list, isNew: true)
        cache.forget("Mikl", on: list)

        #expect(cache.remembered("Mikl", on: list) == nil)
    }

    // MARK: - The shared ranking

    @Test("what is offered comes back best first")
    func suggestionsAreRanked() {
        // Ordering is the server's `history_rank`, compiled in: often bought and
        // recently bought, in that combination. Not the fuzzy score -- a close spelling
        // must not outrank something actually bought every week.
        let now = Date()
        let weekly = Cache.Remembered(
            name: "milk",
            unitID: nil,
            tagIDs: [],
            uses: 50,
            lastUsedAt: Int64(now.addingTimeInterval(-86_400).timeIntervalSince1970)
        )
        let once = Cache.Remembered(
            name: "milk chocolate",
            unitID: nil,
            tagIDs: [],
            uses: 1,
            lastUsedAt: Int64(now.timeIntervalSince1970)
        )

        let offered = QuickAdd.suggest("mil", from: [once, weekly], now: now)
        #expect(offered.first == "milk", "the staple lost to a one-off: \(offered)")
    }

    @Test("nothing that does not match is offered")
    func suggestionsAreFiltered() {
        let remembered = Cache.Remembered(
            name: "bread",
            unitID: nil,
            tagIDs: [],
            uses: 5,
            lastUsedAt: Int64(Date().timeIntervalSince1970)
        )
        #expect(QuickAdd.suggest("milk", from: [remembered]).isEmpty)
    }

    @Test("an empty history offers nothing rather than crashing the boundary")
    func anEmptyHistory() {
        #expect(QuickAdd.suggest("milk", from: []).isEmpty)
    }
}
