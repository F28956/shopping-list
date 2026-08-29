import Foundation
import Testing
import GRDB

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

    // MARK: - Lists this device minted for itself

    /// The watch has no server to mint ids, so every one of its lists has a negative
    /// one -- and `remember(lists:)` spares negative ids when it clears, because on a
    /// phone they mean "made here and not yet sent". Re-inserting a row that is still
    /// there hit the primary key and threw, and a thrown write was swallowed whole: the
    /// watch's picture froze on the first snapshot it ever received and no later one
    /// could correct it.
    @Test("a list written again replaces the row rather than failing")
    func rewritingAListReplacesIt() {
        let cache = Cache.inMemory()
        let mine = List(id: -1, uuid: "abc", name: "Home", ownerID: 0, role: .editor)

        cache.remember(lists: [mine])
        cache.remember(lists: [List(id: -1, uuid: "abc", name: "Household", ownerID: 0, role: .editor)])

        #expect(cache.lists().map(\.name) == ["Household"], "the second snapshot was lost")
    }

    /// And the list itself. `remember(lists:)` spares negative ids, so on the watch --
    /// where every id is negative -- a list deleted on the phone stayed on the wrist,
    /// and a list whose uuid changed appeared twice under the same name.
    @Test("a list the picture no longer mentions is dropped with its rows")
    func departedListsAreDropped() {
        let cache = Cache.inMemory()
        let kept = List(id: -1, uuid: "kept", name: "Home", ownerID: 0, role: .editor)
        let gone = List(id: -2, uuid: "gone", name: "Home", ownerID: 0, role: .editor)
        cache.remember(lists: [kept, gone])
        cache.remember(items: [item(id: -1, name: "Bread")], on: kept)
        cache.remember(items: [item(id: -2, name: "Stale")], on: gone)

        cache.forgetLists(outside: ["kept"])

        #expect(cache.lists().map(\.uuid) == ["kept"], "a departed list stayed")
        #expect(cache.items(on: gone).isEmpty, "its rows stayed with it")
        #expect(cache.items(on: kept).map(\.name) == ["Bread"], "the surviving list lost its rows")
    }

    /// The same failure a list further down: rows whose list has gone, or whose id has
    /// changed, are unreachable from any list and cannot be cleared by a write that
    /// clears by list id.
    @Test("rows belonging to no list are dropped")
    func orphanedRowsAreSwept() {
        let cache = Cache.inMemory()
        let kept = list(id: -1, name: "Home")
        cache.remember(lists: [kept])
        cache.remember(items: [item(id: -1, name: "Bread")], on: kept)
        cache.remember(items: [item(id: -9, name: "Ghost")], on: list(id: 7, name: "Gone"))

        cache.forgetItems(outside: [-1])

        #expect(cache.items(on: kept).map(\.name) == ["Bread"])
        #expect(cache.items(on: list(id: 7, name: "Gone")).isEmpty, "a stranded row survived")
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

    // MARK: - The categories, which belong to no one list

    /// The bug the Mac's settings screen showed: with no lists, the screen for managing
    /// categories opened empty. `allTags` walked `lists()` and stopped, so a device that
    /// had not made a list yet answered "there are none" -- on a fresh install, which is
    /// exactly when somebody goes and looks.
    @Test func categoriesExistBeforeAnyListDoes() {
        let cache = Cache.inMemory()

        #expect(!cache.allTags().isEmpty, "a device with no lists has no vocabulary")
        #expect(
            cache.allTags().count == Reference.tags.count,
            "the shipped categories are not what an untouched device answers"
        )
    }

    /// It used to return a category it had not stored anywhere.
    @Test func aCategoryAddedWithNoListsIsKept() {
        let cache = Cache.inMemory()
        let before = cache.allTags().count

        let made = cache.addTag(named: "Bakery", emoji: "🥐")

        #expect(cache.allTags().count == before + 1)
        #expect(cache.allTags().contains { $0.id == made.id && $0.name == "Bakery" })
    }

    @Test func aCategoryRenamedWithNoListsIsKept() {
        let cache = Cache.inMemory()
        let first = cache.allTags()[0]

        cache.rename(tag: first.id, to: "Cheese counter", emoji: "🧀")

        #expect(cache.allTags().first { $0.id == first.id }?.name == "Cheese counter")
    }

    @Test func aCategoryDeletedWithNoListsStaysDeleted() {
        let cache = Cache.inMemory()
        let first = cache.allTags()[0]

        cache.removeTag(first.id)

        #expect(!cache.allTags().contains { $0.id == first.id })
    }

    /// A list's own order still wins once there is one: the vocabulary is global, the
    /// order is per list, and the two share a table.
    @Test func aListsOwnOrderIsWhatIsRead() {
        let cache = Cache.inMemory()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(
            tags: [
                Tag(id: 90, name: "Last", emoji: nil, sortOrder: 0),
                Tag(id: 91, name: "First", emoji: nil, sortOrder: 1),
            ],
            on: list
        )

        #expect(cache.allTags().map(\.name) == ["Last", "First"])
    }

    /// The shape the server has always had, and the cache did not: one vocabulary, and
    /// an order over it per list. Renaming is one statement against one row, so two
    /// lists cannot come to disagree about what a category is called -- which under the
    /// old per-list copies was prevented only by remembering to loop over all of them.
    @Test func aRenameReachesEveryList() {
        let cache = Cache.inMemory()
        let one = list(id: 1, name: "Home")
        let two = list(id: 2, name: "Boat")
        cache.remember(lists: [one, two])
        let shared = [
            Tag(id: 900, name: "Produce", emoji: "🥬", sortOrder: 0),
            Tag(id: 901, name: "Dairy", emoji: "🧀", sortOrder: 1),
        ]
        cache.remember(tags: shared, on: one)
        cache.remember(tags: shared, on: two)

        cache.rename(tag: 900, to: "Greengrocer", emoji: "🥕")

        #expect(cache.tags(on: one).first { $0.id == 900 }?.name == "Greengrocer")
        #expect(cache.tags(on: two).first { $0.id == 900 }?.name == "Greengrocer")
        #expect(cache.allTags().first { $0.id == 900 }?.emoji == "🥕")
    }

    /// The other half of the split: a list keeps its own order, and reordering one does
    /// not reach the other.
    @Test func eachListKeepsItsOwnOrder() {
        let cache = Cache.inMemory()
        let one = list(id: 1, name: "Home")
        let two = list(id: 2, name: "Boat")
        cache.remember(lists: [one, two])
        let a = Tag(id: 900, name: "Produce", emoji: nil, sortOrder: 0)
        let b = Tag(id: 901, name: "Dairy", emoji: nil, sortOrder: 1)
        cache.remember(tags: [a, b], on: one)
        cache.remember(tags: [b, a], on: two)

        #expect(cache.tags(on: one).map(\.id) == [900, 901])
        #expect(cache.tags(on: two).map(\.id) == [901, 900])
    }

    /// A category added anywhere is a category everywhere, including on a list that has
    /// its own order and has never heard of it.
    @Test func anAddedCategoryReachesAListThatWasNotAskedAboutIt() {
        let cache = Cache.inMemory()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(
            tags: [Tag(id: 900, name: "Produce", emoji: nil, sortOrder: 0)],
            on: list
        )

        let made = cache.addTag(named: "Bakery", emoji: "🥐")

        #expect(cache.allTags().contains { $0.id == made.id })
        #expect(
            cache.tags(on: list).map(\.id).contains(made.id),
            "a list with an order of its own never saw the new category"
        )
        #expect(
            cache.tags(on: list).last?.id == made.id,
            "it did not land at the end of the walk"
        )
    }

    @Test func aRemovedCategoryLeavesNoOrderBehindIt() {
        let cache = Cache.inMemory()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(
            tags: [
                Tag(id: 900, name: "Produce", emoji: nil, sortOrder: 0),
                Tag(id: 901, name: "Dairy", emoji: nil, sortOrder: 1),
            ],
            on: list
        )
        cache.remember(items: [item(id: 1, name: "Milk")], on: list)

        cache.removeTag(901)

        #expect(cache.allTags().map(\.id) == [900])
        #expect(cache.tags(on: list).map(\.id) == [900], "the order still names a category that is gone")
    }

    /// A list's answer is evidence about the vocabulary, not the whole of it. A list
    /// carrying a subset must not delete the categories it does not mention.
    @Test func onelistsAnswerDoesNotNarrowTheVocabulary() {
        let cache = Cache.inMemory()
        let one = list(id: 1, name: "Home")
        let two = list(id: 2, name: "Boat")
        cache.remember(lists: [one, two])
        cache.remember(
            tags: [
                Tag(id: 900, name: "Produce", emoji: nil, sortOrder: 0),
                Tag(id: 901, name: "Dairy", emoji: nil, sortOrder: 1),
            ],
            on: one
        )

        cache.remember(tags: [Tag(id: 900, name: "Produce", emoji: nil, sortOrder: 0)], on: two)

        #expect(cache.allTags().count == 2, "a list that mentioned one category deleted the other")
    }

    /// Signing out takes the vocabulary with it: it is one server's, and the next person
    /// on this device is a different person.
    @Test func signingOutForgetsTheCategories() {
        let cache = Cache.inMemory()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(tags: [Tag(id: 900, name: "Produce", emoji: nil, sortOrder: 0)], on: list)

        cache.forgetEverything()

        #expect(!cache.allTags().contains { $0.id == 900 })
        #expect(cache.allTags().count == Reference.tags.count, "the shipped set did not come back")
    }

    /// The memory belongs to the *list*, not to whoever is signed in.
    ///
    /// The server moved it there so a household shares one, and hands it back per list
    /// like everything else. This kept it on sign-out anyway, on reasoning that had
    /// expired -- so the next person on a shared device got the previous person's
    /// shopping suggested to them, measured and filed, under names they never typed.
    @Test("signing out forgets what the lists taught the box")
    func signingOutForgetsTheHistory() {
        let cache = Cache.inMemory()
        let list = list()
        cache.remember(lists: [list])
        cache.remember(item(id: 1, name: "Gin"), on: list, isNew: true)
        #expect(!cache.history(on: list).isEmpty, "the fixture never landed")

        cache.forgetEverything()

        #expect(cache.history(on: list).isEmpty, "a stranger's shopping was kept")
    }

    /// A list made offline carries everything to its real id.
    ///
    /// Two faults, and the second hid the first. `history` was never in the list of
    /// tables to move and `reference` had stopped holding the tag order at v8, so both
    /// would have been orphaned under a number nothing pointed at any more. But every
    /// statement was also handed three arguments while only the first names three,
    /// which GRDB refuses -- and the refusal rolled back the transaction, so this moved
    /// *nothing*. Not the items, not the queue, not the list's own id.
    ///
    /// Nothing called it in a test, and `Cache.write` swallowed the throw. The visible
    /// end of it: a list made offline stayed in the cache under its negative id, which
    /// `remember(lists:)` spares, while the server's answer arrived as a second row --
    /// the same list twice, one holding the shopping and one empty.
    @Test("adopting a server id brings the history and the walking order along")
    func adoptingCarriesTheMemory() {
        let cache = Cache.inMemory()
        let mine = List(id: -1, uuid: "abc", name: "Home", ownerID: 0, role: .owner)
        let real = List(id: 42, uuid: "abc", name: "Home", ownerID: 7, role: .owner)
        cache.remember(lists: [mine])
        cache.remember(
            tags: [Tag(id: 900, name: "dairy", emoji: nil, sortOrder: 0)],
            on: mine
        )
        cache.remember(
            Item(id: 5, uuid: "milk", name: "Milk", amount: 4,
                 unitID: 2, doneAt: nil, tagIDs: [900]),
            on: mine,
            isNew: true
        )

        cache.adopt(mine, as: real)

        #expect(cache.lists().map(\.id) == [42], "the list row itself did not move")
        let carried = cache.history(on: real)
        #expect(carried.map(\.name) == ["milk"], "the memory stayed behind")
        #expect(carried.first?.amount == 4, "how much was forgotten on the way")
        #expect(cache.history(on: mine).isEmpty, "it was copied rather than moved")
        #expect(
            cache.tags(on: real).contains { $0.id == 900 },
            "the walking order stayed behind"
        )
    }

    /// The migration off the old per-list copies, driven against a database in the
    /// shape a real device is in: one list, twenty-one categories written under it.
    @Test func theOldPerListCopiesBecomeOneVocabulary() throws {
        let path = NSTemporaryDirectory() + "v8-\(UUID().uuidString).sqlite"
        defer { try? FileManager.default.removeItem(atPath: path) }

        // Built by hand in the pre-v8 shape, because that is the state on disk that has
        // to survive -- a cache made by today's code would already be migrated.
        let old = try DatabaseQueue(path: path)
        try old.write { db in
            try db.execute(sql: """
                CREATE TABLE grdb_migrations (identifier TEXT NOT NULL PRIMARY KEY);
                CREATE TABLE reference (
                    kind TEXT NOT NULL, list_id INTEGER NOT NULL, id INTEGER NOT NULL,
                    name TEXT NOT NULL, emoji TEXT, position INTEGER NOT NULL,
                    bare BOOLEAN NOT NULL DEFAULT 0,
                    PRIMARY KEY (kind, list_id, id)
                );
                """)
            for row in ["v1", "v2-outbox", "v3-history", "v4-history-amount",
                        "v5-units-that-stand-alone", "v6-reread-units", "v7-history-display"] {
                try db.execute(sql: "INSERT INTO grdb_migrations VALUES (?)", arguments: [row])
            }
            try db.execute(sql: """
                CREATE TABLE lists (id INTEGER PRIMARY KEY, uuid TEXT NOT NULL, name TEXT NOT NULL,
                                    owner_id INTEGER NOT NULL, role TEXT NOT NULL, position INTEGER NOT NULL);
                CREATE TABLE items (id INTEGER NOT NULL, list_id INTEGER NOT NULL, uuid TEXT NOT NULL,
                                    name TEXT NOT NULL, amount DOUBLE, unit_id INTEGER, done_at DOUBLE,
                                    tag_ids TEXT NOT NULL, position INTEGER NOT NULL, PRIMARY KEY (list_id, id));
                CREATE TABLE history (list_id INTEGER NOT NULL, name TEXT NOT NULL, display TEXT NOT NULL DEFAULT '',
                                      unit_id INTEGER, amount DOUBLE, tag_ids TEXT NOT NULL,
                                      uses INTEGER NOT NULL, last_used_at INTEGER NOT NULL,
                                      PRIMARY KEY (list_id, name));
                """)
            try db.execute(sql: "INSERT INTO lists VALUES (-1, 'u', 'Home', 0, 'owner', 0)")
            for (at, name) in ["tesco", "produce", "dairy"].enumerated() {
                try db.execute(
                    sql: "INSERT INTO reference VALUES ('tag', -1, ?, ?, NULL, ?, 0)",
                    arguments: [at + 1, name, at]
                )
            }
        }
        try old.close()

        // Opening it is what migrates it.
        let cache = Cache(path: path)

        #expect(cache.allTags().map(\.name) == ["tesco", "produce", "dairy"],
                "the vocabulary did not survive the move off per-list rows")
        #expect(cache.tags(on: List(id: -1, uuid: "u", name: "Home", ownerID: 0, role: .owner))
                    .map(\.name) == ["tesco", "produce", "dairy"],
                "the list lost the order it had")
    }
}
