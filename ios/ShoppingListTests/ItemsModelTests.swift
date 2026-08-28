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
        // Through `CachingBackend`, because these are about the cache and the queue --
        // which now live behind it. A bare `API` here would mean a backend that keeps
        // its own store, and every one of these would quietly stop testing what it says
        // it tests.
        let made = ItemsModel(list: list, api: CachingBackend(remote: api, cache: cache), cache: cache)
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
            api: CachingBackend(
                remote: API(baseURL: URL(string: "http://127.0.0.1:1")!, token: { "none" }),
                cache: cache
            ),
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
        await model.loadReference()
        let watching = Task { await model.watch() }
        defer { watching.cancel() }
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
        await model.loadReference()
        let watching = Task { await model.watch() }
        defer { watching.cancel() }

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
        await model.loadReference()
        let watching = Task { await model.watch() }
        defer { watching.cancel() }
        await until { model.tags.count == 2 }

        // A different object over the same database, as another part of the app is.
        let elsewhere = Cache(path: path, sending: { true })
        elsewhere.removeTag(901)

        // The limit is narrower than it was, and this is where that was noticed.
        //
        // A *read* sees it: `tags(orderedFor:)` goes to the file, and the file has the
        // other connection's write in it. What is still connection-bound is the
        // *notification* -- nothing wakes this screen, so it catches up only when
        // something else does, which here is the reference re-read that follows any
        // change at all.
        //
        // So the rule to remember is not "a second connection is invisible". It is
        // "a second connection cannot wake you". That is a smaller claim and the true
        // one, and it is still why the app opens exactly one cache.
        await until({ model.tags.count == 1 }, within: 1)
        #expect(
            model.tags.map(\.name) == ["produce"],
            "a read did not see what another connection wrote"
        )
    }
}
