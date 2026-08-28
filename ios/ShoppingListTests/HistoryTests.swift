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
        amount: Double = 1,
        tags: [Int64] = []
    ) -> Item {
        Item(
            id: 1,
            uuid: "item-\(name)",
            name: name,
            amount: amount,
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
            display: "Milk",
            unitID: nil,
            amount: nil,
            tagIDs: [],
            uses: 50,
            lastUsedAt: Int64(now.addingTimeInterval(-86_400).timeIntervalSince1970)
        )
        let once = Cache.Remembered(
            name: "milk chocolate",
            display: "Milk chocolate",
            unitID: nil,
            amount: nil,
            tagIDs: [],
            uses: 1,
            lastUsedAt: Int64(now.timeIntervalSince1970)
        )

        let offered = QuickAdd.suggest("mil", from: [once, weekly], now: now)
        #expect(offered.first == "Milk", "the staple lost to a one-off: \(offered)")
    }

    @Test("nothing that does not match is offered")
    func suggestionsAreFiltered() {
        let remembered = Cache.Remembered(
            name: "bread",
            display: "Bread",
            unitID: nil,
            amount: nil,
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

    @Test("how much of it you buy is remembered too")
    func remembersHowMuch() {
        // The half somebody notices: the memory already knew `apples` came in kilos
        // and then asked how many, every week, for something bought two kilos at a
        // time every week.
        let cache = Cache.inMemory()
        cache.remember(item("apples", unit: 2, amount: 2), on: list, isNew: true)

        #expect(cache.remembered("apples", on: list)?.amount == 2)
    }

    @Test("a line that states an amount outranks the memory")
    func aStatedAmountWins() {
        let cache = Cache.inMemory()
        cache.remember(item("apples", unit: 2, amount: 2), on: list, isNew: true)
        let remembered = cache.remembered("apples", on: list)

        let units = [Unit(id: 2, name: "kg")]
        guard case .new(let row) = QuickAdd.resolve(
            "1 kg apples",
            units: units,
            rows: [],
            history: remembered.map { [$0] } ?? []
        ) else {
            Issue.record("expected a new row")
            return
        }
        #expect(row.amount == 1)
    }

    @Test("a bare name gets how much you usually buy")
    func aBareNameGetsTheRememberedAmount() {
        let cache = Cache.inMemory()
        cache.remember(item("apples", unit: 2, amount: 2), on: list, isNew: true)
        let remembered = cache.remembered("apples", on: list)

        guard case .new(let row) = QuickAdd.resolve(
            "apples",
            units: [Unit(id: 2, name: "kg")],
            rows: [],
            history: remembered.map { [$0] } ?? []
        ) else {
            Issue.record("expected a new row")
            return
        }
        #expect(row.amount == 2, "how much was forgotten")
        #expect(row.unitID == 2)
    }
}

/// Reading a unit written with no number in front of it.
///
/// The rule and the data both come from the shared core — `parsing::quick_add` decides,
/// and each unit's `bare` says whether it may. These check the crossing and the two
/// answers that matter.
struct BareUnitTests {
    private let units = [
        Unit(id: 1, name: "unit", bare: false),
        Unit(id: 3, name: "pint", bare: true),
        Unit(id: 8, name: "can", bare: false),
    ]

    @Test("a unit that stands alone is read as one")
    func aBareUnitIsAUnit() {
        guard case .new(let row) = QuickAdd.resolve(
            "pint milk",
            units: units,
            rows: [],
            history: []
        ) else {
            Issue.record("expected a new row")
            return
        }
        // Capitalised, as the server would store it -- see `parsing::capitalise`.
        #expect(row.name == "Milk")
        #expect(row.amount == 1)
        #expect(row.unitID == 3)
    }

    @Test("a name that begins with a unit that does not stand alone is left alone")
    func canOpenerIsNotOneCanOfOpener() {
        guard case .new(let row) = QuickAdd.resolve(
            "can opener",
            units: units,
            rows: [],
            history: []
        ) else {
            Issue.record("expected a new row")
            return
        }
        #expect(row.name == "Can opener", "it was read as a quantity")
        #expect(row.unitID == 1, "it should be counted, not measured in cans")
    }

    @Test("a unit is decoded as not standing alone when the server does not say")
    func anOlderServerSaysNothing() throws {
        // A server that predates the column. Swift's synthesised decoder ignores
        // default values and throws on a missing key, so without the explicit
        // initialiser this would fail to decode at all rather than defaulting.
        let json = Data(#"{"id": 3, "name": "pint"}"#.utf8)
        let unit = try JSONDecoder().decode(Unit.self, from: json)
        #expect(unit.bare == false)
    }
}

/// What the cache must carry into the shared rules.
///
/// The rules live in one place — `parsing::add` — but each platform hand-carries the
/// data into them, and a field dropped on the way is a wrong answer from a right rule.
/// That is exactly what happened: the reference table held a name and an emoji, `bare`
/// was dropped writing units and read back false for all of them, and `pint milk`
/// became an item called "pint milk" on every device that had cached its units.
///
/// So these are round trips. Anything the shared decision reads must survive one.
struct CacheCarriesTheInputsTests {
    private let list = List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)

    @Test("a unit that stands alone still does after a round trip")
    func unitsKeepTheirBareFlag() {
        let cache = Cache.inMemory()
        cache.remember(units: [
            Unit(id: 1, name: "unit", bare: false),
            Unit(id: 3, name: "pint", bare: true),
        ])

        let read = cache.units()
        #expect(read.first { $0.name == "pint" }?.bare == true, "the flag was dropped")
        #expect(read.first { $0.name == "unit" }?.bare == false)
    }

    @Test("what the cache carries is enough to read a line the same way twice")
    func aLineReadsTheSameThroughTheCache() {
        // The end-to-end version of the above, through the shared decision rather than
        // through a field: `pint milk` must mean the same after a round trip as before.
        let cache = Cache.inMemory()
        let units = [Unit(id: 1, name: "unit", bare: false), Unit(id: 3, name: "pint", bare: true)]
        cache.remember(units: units)

        guard case .new(let direct) = QuickAdd.resolve(
            "pint milk", units: units, rows: [], history: []
        ), case .new(let throughCache) = QuickAdd.resolve(
            "pint milk", units: cache.units(), rows: [], history: []
        ) else {
            Issue.record("expected new rows")
            return
        }

        #expect(direct.name == throughCache.name)
        #expect(direct.unitID == throughCache.unitID)
        #expect(throughCache.name == "Milk", "the cache changed what the line means")
        #expect(throughCache.unitID == 3)
    }

    @Test("aisles keep their glyph and order through a round trip")
    func tagsKeepTheirEmoji() {
        let cache = Cache.inMemory()
        cache.remember(
            tags: [
                Tag(id: 40, name: "dairy", emoji: "🧀", sortOrder: 0),
                Tag(id: 10, name: "produce", emoji: nil, sortOrder: 1),
            ],
            on: list
        )

        let read = cache.tags(on: list)
        #expect(read.map(\.name) == ["dairy", "produce"], "the order moved")
        #expect(read.first?.emoji == "🧀")
    }
}

/// The bundled reference data, which is what a device with no server resolves against.
struct BundledReferenceTests {
    @Test("the units that ship with the app know which of them stand alone")
    func theBundleCarriesTheFlag() {
        // Read from the app's own bundle, so this fails if the resource is stale or the
        // decoder drops the field -- both of which have happened.
        let pint = Reference.units.first { $0.name == "pint" }
        #expect(pint != nil, "no pint in the bundled units")
        #expect(pint?.bare == true, "the bundle says pint cannot stand alone")
        #expect(Reference.units.first { $0.name == "unit" }?.bare == false)
    }
}

/// Looking a name up in the memory.
struct RecallTests {
    private let list = List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)

    private func remembered(_ name: String, unit: Int64?, tags: [Int64]) -> Cache.Remembered {
        Cache.Remembered(
            name: name,
            display: name,
            unitID: unit,
            amount: nil,
            tagIDs: tags,
            uses: 1,
            lastUsedAt: 0
        )
    }

    @Test("a line with a quantity still finds what it names")
    func aQuantityDoesNotHideTheName() {
        // The bug: the history was looked up by what somebody *typed*, so this went
        // looking for a memory of "2 kg apples" and found nothing. History applied to
        // bare words and to nothing else, which is why a re-added `2 kg apples` came
        // back unfiled while a re-added `apples` came back filed.
        let history = [remembered("apples", unit: 19, tags: [7])]

        guard case .new(let row) = QuickAdd.resolve(
            "2 kg apples",
            units: [Unit(id: 19, name: "kg", bare: true)],
            rows: [],
            history: history
        ) else {
            Issue.record("expected a new row")
            return
        }
        #expect(row.tagIDs == [7], "the aisle it is always filed under was not applied")
    }

    @Test("a name is written and read under the same key")
    func theFoldIsOneFold() {
        // Two folds drifted here: the store lowercased and the lookup lowercased, but
        // neither trimmed the same way, so a name with a stray space was written under
        // one key and looked for under another.
        let cache = Cache.inMemory()
        cache.remember(
            Item(id: 1, uuid: "u", name: "  Milk ", amount: 1, unitID: 4, doneAt: nil, tagIDs: []),
            on: list,
            isNew: true
        )

        #expect(cache.remembered("milk", on: list)?.unitID == 4)
        #expect(cache.remembered("  MILK  ", on: list)?.unitID == 4)
        #expect(cache.history(on: list).count == 1, "it was written twice")
    }
}
