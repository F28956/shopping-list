import Foundation
import Testing

@testable import ShoppingList

/// The queue of changes the server has not been told about yet.
///
/// In-memory throughout, so the queue on the machine running these is neither read nor
/// emptied.
struct OutboxTests {

    private let list = List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)

    private func item(_ id: Int64, _ name: String) -> Item {
        Item(id: id, uuid: "item-\(id)", name: name, amount: 1, unitID: nil, doneAt: nil, tagIDs: [])
    }

    @Test func aTickIsQueuedWithWhatItNeedsToBeReplayed() {
        let outbox = Cache.inMemory().outbox

        outbox.setDone(item(7, "Milk"), on: list, done: true)

        let queued = outbox.all()
        #expect(queued.count == 1)
        #expect(queued[0].kind == QueuedOperation.Kind.setDone)
        #expect(queued[0].itemID == 7)
        #expect(queued[0].listID == 1)
        // Both names for the row: the id the REST routes want today, and the uuid the
        // sync route will want. Carried from the first day so the table needs no
        // migration when that lands.
        #expect(queued[0].itemUUID == "item-7")
        #expect(queued[0].listUUID == "list-1")
        #expect(queued[0].done)
    }

    /// A device's own changes replay in the order they were made, always. The sequence
    /// is the row id, so it can only count up.
    @Test func theOrderIsTheOrderTheyWereMade() {
        let outbox = Cache.inMemory().outbox

        outbox.setDone(item(1, "Milk"), on: list, done: true)
        outbox.setDone(item(2, "Bread"), on: list, done: true)
        outbox.setDone(item(1, "Milk"), on: list, done: false)

        let queued = outbox.all()
        #expect(queued.map(\.itemID) == [1, 2, 1])
        #expect(queued.map(\.done) == [true, true, false])
        #expect(queued[0].sequence < queued[1].sequence)
        #expect(queued[1].sequence < queued[2].sequence)
    }

    /// Every operation is named, and no two share a name. The sync route recognises a
    /// resend by this, so a collision would be a change silently swallowed.
    @Test func everyOperationHasItsOwnName() {
        let outbox = Cache.inMemory().outbox

        for id in Int64(1)...20 {
            outbox.setDone(item(id, "Thing \(id)"), on: list, done: true)
        }

        let names = Set(outbox.all().map(\.id))
        #expect(names.count == 20)
        #expect(!names.contains(""))
    }

    @Test func theQueueIsScopedWhenAskedAboutOneList() {
        let cache = Cache.inMemory()
        let other = List(id: 2, uuid: "list-2", name: "Bakery", ownerID: 9, role: .editor)

        cache.outbox.setDone(item(1, "Milk"), on: list, done: true)
        cache.outbox.setDone(item(2, "Bread"), on: other, done: true)

        #expect(cache.outbox.forList(list).map(\.itemID) == [1])
        #expect(cache.outbox.forList(other).map(\.itemID) == [2])
        #expect(cache.outbox.waiting == 2, "and both are still waiting overall")
    }

    /// Signing out empties it. What is in there are changes to somebody else's lists,
    /// made by somebody who is no longer here.
    @Test func signingOutEmptiesTheQueue() {
        let cache = Cache.inMemory()
        cache.outbox.setDone(item(1, "Milk"), on: list, done: true)

        cache.forgetEverything()

        #expect(cache.outbox.all().isEmpty)
    }

    /// The queue outlives the app, which is the only reason it is worth having: a tick
    /// made in a shop has to still be there when the phone comes out of a pocket.
    @Test func theQueueSurvivesBeingReopened() throws {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }

        let path = folder.appendingPathComponent("queue.sqlite").path
        let first = Cache(path: path)
        first.outbox.setDone(item(3, "Coffee"), on: list, done: true)

        let second = Cache(path: path)

        #expect(second.outbox.all().map(\.itemID) == [3])
    }
}
