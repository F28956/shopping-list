import Foundation
import Testing

@testable import ShoppingList

/// The queue of changes the server has not been told about yet.
///
/// In-memory throughout, so the queue on the machine running these is neither read nor
/// emptied.
struct OutboxTests {

    private let list = List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)

    private func item(_ id: Int64, _ name: String, amount: Double = 1, unit: Int64? = nil) -> Item {
        Item(
            id: id,
            uuid: "item-\(id)",
            name: name,
            amount: amount,
            unitID: unit,
            doneAt: nil,
            tagIDs: []
        )
    }

    @Test func aTickIsQueuedWithWhatItNeedsToBeReplayed() {
        let outbox = Cache.inMemory().outbox

        outbox.setDone(item(7, "Milk"), on: list, done: true)

        let queued = outbox.all()
        #expect(queued.count == 1)
        #expect(queued[0].kind == QueuedOperation.Kind.setDone)
        #expect(queued[0].itemID == 7)
        #expect(queued[0].listID == 1)
        // Both names for the row: the uuid, which is the only one that travels, and
        // the id, which stays here to key the screen on.
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

    /// Every kind reaches the wire as the route expects, which is the one translation
    /// in this file with somewhere to go wrong: the arguments live as JSON in a column,
    /// and they have to come back out in the right fields.
    @Test func eachKindReachesTheWireIntact() throws {
        let cache = Cache.inMemory()
        let milk = item(1, "Milk")
        // Measured, so the `seen` fields have something to carry that is not a default.
        let measured = item(1, "Milk", amount: 2, unit: 3)

        cache.outbox.add(uuid: "new-1", localID: -99, line: "2 kg apples", on: list)
        cache.outbox.setDone(milk, on: list, done: true)
        cache.outbox.update(measured, on: list, name: "Whole milk", amount: 3, unitID: 4)
        cache.outbox.delete(milk, on: list)
        cache.outbox.clearDone([milk, item(2, "Bread")], on: list)

        let wire = cache.outbox.all().map(\.onTheWire)
        #expect(wire.map(\.kind) == ["add", "set_done", "update", "delete", "clear_done"])

        #expect(wire[0].item == "new-1")
        #expect(wire[0].line == "2 kg apples")

        #expect(wire[1].done == true)

        #expect(wire[2].name == "Whole milk")
        #expect(wire[2].amount == 3)
        #expect(wire[2].unitID == 4)
        // What the row looked like when the edit was made — the thing that decides
        // between renaming a row and splitting one.
        #expect(wire[2].seen?.name == "Milk")
        #expect(wire[2].seen?.amount == 2)
        #expect(wire[2].seen?.unitID == 3)

        // A sweep is about a list, not a row: no item, and the rows it meant by name.
        #expect(wire[4].item == nil)
        #expect(wire[4].items == ["item-1", "item-2"])
    }

    /// The JSON is what the server reads, so it is worth looking at rather than trusting
    /// the field names to line up.
    @Test func theBatchEncodesTheWayTheRouteReadsIt() throws {
        let cache = Cache.inMemory()
        cache.outbox.update(
            item(1, "Milk", amount: 2, unit: 3),
            on: list,
            name: "Whole milk",
            amount: 3,
            unitID: 4
        )

        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let data = try encoder.encode(SyncBatch(operations: cache.outbox.all().map(\.onTheWire)))
        let json = try #require(
            try JSONSerialization.jsonObject(with: data) as? [String: Any]
        )
        let operation = try #require((json["operations"] as? [[String: Any]])?.first)

        #expect(operation["kind"] as? String == "update")
        #expect(operation["unit_id"] as? Int == 4, "snake_case, as the route reads it")
        #expect((operation["seen"] as? [String: Any])?["unit_id"] as? Int == 3)
        // RFC 3339, which is what the route's `at` decodes.
        let at = try #require(operation["at"] as? String)
        #expect(at.contains("T"))
    }

    /// A row made offline is queued under a uuid and a placeholder id, and the uuid is
    /// the only one that travels. The negative id never leaves the device.
    @Test func aRowMadeOfflineTravelsUnderItsUuid() {
        let cache = Cache.inMemory()

        cache.outbox.add(uuid: "minted", localID: -1234, line: "Bread", on: list)

        let queued = cache.outbox.all()[0]
        #expect(queued.itemID == -1234)
        #expect(queued.onTheWire.item == "minted")
    }

    // MARK: - Filing

    @Test("filing is queued rather than sent, so it survives having no server")
    func filingIsQueued() {
        let outbox = Cache.inMemory().outbox
        let milk = item(7, "Milk")

        outbox.tag(milk, on: list, tagID: 5, attached: true)
        outbox.tag(milk, on: list, tagID: 3, attached: false)

        let queued = outbox.forList(list)
        #expect(queued.count == 2)
        #expect(queued[0].kind == QueuedOperation.Kind.attachTag)
        #expect(queued[0].tagID == 5)
        #expect(queued[1].kind == QueuedOperation.Kind.detachTag)
        #expect(queued[1].tagID == 3)
    }

    @Test("the wire calls it tag_id, which is the one thing the server insists on")
    func filingOnTheWire() throws {
        // The two tag kinds are built by hand at both ends, so nothing but a test
        // notices if the key is spelled the way Swift spells the property. A phone in
        // a shop would notice, eventually, by the filing never arriving.
        let outbox = Cache.inMemory().outbox
        outbox.tag(item(7, "Milk"), on: list, tagID: 5, attached: true)

        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let wire = try encoder.encode(outbox.forList(list)[0].onTheWire)
        let fields = try #require(
            try JSONSerialization.jsonObject(with: wire) as? [String: Any]
        )

        #expect(fields["kind"] as? String == "attach_tag")
        #expect((fields["tag_id"] as? NSNumber)?.int64Value == 5)
        #expect(fields["item"] as? String == "item-7")
    }
}
