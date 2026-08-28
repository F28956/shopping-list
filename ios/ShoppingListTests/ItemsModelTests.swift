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
    /// Waits for the observation to deliver, or gives up and lets the expectation fail.
    ///
    /// A poll rather than a single `Task.yield()`, because the update is genuinely
    /// asynchronous now: a write wakes `ValueObservation`, which re-runs its fetch on
    /// the database's own queue and hands the result back on the main one. Two hops. A
    /// test that yields once is testing the scheduler, not the behaviour.
    private func until(
        _ settled: () -> Bool,
        within seconds: Double = 2
    ) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !settled() && Date() < deadline {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    private func model(_ cache: Cache = .inMemory(sending: { true })) -> (ItemsModel, Cache, List) {
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

    /// The one that was reported: `dairy` removed in Settings, still on the rows of a
    /// list that was already open.
    ///
    /// Nothing was stale in the database. The screen held a copy taken when it loaded,
    /// the four functions that change a category never said they had changed anything,
    /// and the one thing listening re-read the items and not the categories. Three
    /// independent ways to be wrong about the same fact.
    @Test("a category removed anywhere leaves the rows of an open list")
    func removingACategoryReachesAnOpenList() async {
        let cache = Cache.inMemory()
        let (model, _, list) = model(cache)
        cache.remember(
            tags: [
                Tag(id: 900, name: "produce", emoji: "🥬", sortOrder: 0),
                Tag(id: 901, name: "dairy", emoji: "🧀", sortOrder: 1),
            ],
            on: list
        )
        model.reloadFromCache()
        #expect(model.tags.map(\.name).contains("dairy"), "the screen never had it to lose")

        // What the Categories screen does, and nothing more. No reload is asked for
        // here: the point is that the model hears about it by itself.
        cache.removeTag(901)
        await until { !model.tags.map(\.name).contains("dairy") }

        #expect(
            !model.tags.map(\.name).contains("dairy"),
            "a category removed in Settings is still on an open list"
        )
    }

    /// Same shape, for a rename: the row should say what the database says.
    @Test("a category renamed anywhere reaches an open list")
    func renamingACategoryReachesAnOpenList() async {
        let cache = Cache.inMemory()
        let (model, _, list) = model(cache)
        cache.remember(
            tags: [Tag(id: 900, name: "dairy", emoji: "🧀", sortOrder: 0)],
            on: list
        )
        model.reloadFromCache()

        cache.rename(tag: 900, to: "cheese counter", emoji: "🧀")
        await until { model.tags.first { $0.id == 900 }?.name == "cheese counter" }

        #expect(model.tags.first { $0.id == 900 }?.name == "cheese counter")
    }

    /// Where the observation stops seeing, written down so it is a known limit rather
    /// than a later surprise.
    ///
    /// A `DatabaseQueue` observes writes made through *itself*. A second `Cache` over
    /// the same file is a second connection, and this one never hears about it. The app
    /// is safe because it opens exactly one -- `Cache.shared`, which every screen, the
    /// watch link and the outbox go through -- and this test exists to fail loudly if
    /// somebody ever assumes more than that.
    ///
    /// If a share extension, a widget or a background refresh is ever added, this is the
    /// test that says what has to change: a `DatabasePool` and cross-process
    /// notification, not another hand-posted `cacheChanged`.
    @Test("a second connection is not observed, and that is the known limit")
    func aSecondConnectionIsNotObserved() async {
        let path = NSTemporaryDirectory() + "observe-\(UUID().uuidString).sqlite"
        defer { try? FileManager.default.removeItem(atPath: path) }

        let mine = Cache(path: path, sending: { true })
        let (model, _, list) = model(mine)
        mine.remember(
            tags: [
                Tag(id: 900, name: "produce", emoji: nil, sortOrder: 0),
                Tag(id: 901, name: "dairy", emoji: nil, sortOrder: 1),
            ],
            on: list
        )
        await until { model.tags.count == 2 }

        // A different object over the same database, as another part of the app is.
        let elsewhere = Cache(path: path, sending: { true })
        elsewhere.removeTag(901)

        // Deliberately asserting the limit. If this ever starts failing because the
        // screen *did* catch up, the comment above is out of date and the restriction on
        // opening a second cache can be lifted.
        await until({ model.tags.count == 1 }, within: 0.5)
        #expect(
            model.tags.map(\.name) == ["produce", "dairy"],
            "a second connection is now observed -- see the note on Cache.observe(list:)"
        )
    }
}
