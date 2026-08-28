import Testing
import Foundation
@testable import ShoppingList

/// The list's logic, tested at last.
///
/// None of this could be reached before: it lived inside `ItemsView`, so getting at
/// `withUnsent` or a drain's outcome meant hosting a view, and nothing did. The rules
/// about what a queue does with a refusal were verified by hand or not at all.
///
/// In memory throughout, and with no server: `API` against an address nothing answers
/// is exactly the standalone case, which is what these are about.
@MainActor
struct ItemsModelTests {
    private func model(_ cache: Cache = .inMemory()) -> (ItemsModel, Cache, List) {
        let list = List(id: -1, uuid: "list-1", name: "Shop", ownerID: 0, role: .owner)
        cache.remember(lists: [list])
        cache.remember(units: [
            Unit(id: 1, name: "unit", bare: false),
            Unit(id: 19, name: "kg", bare: true),
        ])
        let api = API(
            baseURL: URL(string: "http://127.0.0.1:1")!,
            token: { "none" }
        )
        let made = ItemsModel(list: list, api: api, cache: cache)
        made.units = cache.units()
        return (made, cache, list)
    }

    @Test("a typed line becomes a row that is read, not echoed")
    func addingReadsTheLine() async {
        let (model, _, _) = model()
        model.line = "2 kg apples"
        await model.add()

        #expect(model.items.count == 1)
        #expect(model.items.first?.name == "Apples")
        #expect(model.items.first?.amount == 2)
        #expect(model.items.first?.unitID == 19)
        #expect(model.line == "", "the field was not cleared for the next item")
    }

    @Test("adding the same thing twice makes one row")
    func addingIsIdempotent() async {
        let (model, _, _) = model()
        model.line = "2 kg apples"
        await model.add()
        model.line = "2 KG Apples"
        await model.add()

        #expect(model.items.count == 1, "a second row appeared for one intention")
        #expect(model.items.first?.amount == 2, "the amount moved")
    }

    @Test("adding something crossed off brings it back")
    func addingPutsSomethingBack() async {
        let (model, _, _) = model()
        model.line = "apples"
        await model.add()
        await model.toggle(model.items[0])
        #expect(model.items[0].isDone)

        model.line = "apples"
        await model.add()

        #expect(model.items.count == 1)
        #expect(!model.items[0].isDone, "it did not come back")
    }

    @Test("a tick with nowhere to send it is still a tick")
    func tickingWithNoServer() async {
        let (model, cache, list) = model()
        model.line = "milk"
        await model.add()

        await model.toggle(model.items[0])

        #expect(model.items[0].isDone)
        #expect(cache.items(on: list).first?.isDone == true, "it was not written down")
    }

    @Test("what is queued is laid back over what a server answered")
    func unsentWorkSurvivesAReload() async {
        // The rule that stops a successful load visibly undoing a tick that is still
        // queued: the server has not been told, so it answers with the old state.
        let (model, _, list) = model()
        model.line = "milk"
        await model.add()
        await model.toggle(model.items[0])

        let asTheServerSeesIt = [
            Item(
                id: 7,
                uuid: model.items[0].uuid,
                name: "Milk",
                amount: 1,
                unitID: 1,
                doneAt: nil,
                tagIDs: []
            )
        ]

        let merged = model.withUnsent(asTheServerSeesIt)
        #expect(merged.first?.isDone == true, "the queued tick was undone by a reload")
        _ = list
    }

    @Test("rows this device made and has not sent are carried across a reload")
    func locallyMadeRowsSurvive() async {
        let (model, _, _) = model()
        model.line = "milk"
        await model.add()

        // The server has never heard of it, so it says nothing about it.
        let merged = model.withUnsent([])
        #expect(merged.count == 1, "a row made offline vanished on the first load")
    }

    @Test("a row somebody else deleted does not come back as a ghost")
    func deletedRowsDoNotReturn() async {
        // Any queued operation used to qualify a row for carrying across, which meant
        // a tick queued against a row somebody else had deleted put it back on screen
        // -- present here, gone everywhere else, impossible to be rid of.
        let (model, cache, list) = model()
        let theirs = Item(
            id: 3,
            uuid: "theirs",
            name: "Bread",
            amount: 1,
            unitID: 1,
            doneAt: nil,
            tagIDs: []
        )
        cache.remember(items: [theirs], on: list)
        model.items = [theirs]
        await model.toggle(theirs)

        let merged = model.withUnsent([])
        #expect(merged.isEmpty, "a deleted row came back")
    }

    @Test("clearing takes only the rows this screen could see")
    func clearingIsBounded() async {
        let (model, _, _) = model()
        model.line = "milk"
        await model.add()
        model.line = "bread"
        await model.add()
        await model.toggle(model.items[0])

        await model.clearDone()

        #expect(model.items.count == 1)
        #expect(model.items.first?.name == "Bread")
    }

    @Test("the categories fall back to the bundled set with no server")
    func referenceFallsBack() async {
        // A cache with nothing in it, which is a first run. The helper above seeds two
        // units so the other tests can parse, and a seeded cache is the authority --
        // the bundle is a first run and nothing else.
        let cache = Cache.inMemory()
        let list = List(id: -1, uuid: "list-1", name: "Shop", ownerID: 0, role: .owner)
        cache.remember(lists: [list])
        let model = ItemsModel(
            list: list,
            api: API(baseURL: URL(string: "http://127.0.0.1:1")!, token: { "none" }),
            cache: cache
        )

        await model.loadReference()

        #expect(!model.tags.isEmpty, "no categories, so nothing can be filed")
        #expect(
            model.units.contains { $0.name == "pint" && $0.bare },
            "the bundled units arrived without knowing which stand alone"
        )
    }
}
