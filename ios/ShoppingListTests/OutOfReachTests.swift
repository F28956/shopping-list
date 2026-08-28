import Foundation
import Testing

@testable import ShoppingList

/// A device out of reach of wherever its changes are going.
///
/// The two promises the app is built on, tested at the layer every client shares --
/// `Cache`, `Outbox` and `Destination`. The watch drains to its phone, the phone drains
/// to a server, and the rules are identical either way; that is what `Destination` is
/// for. What is *not* covered here is the watch's screens, which have no test target --
/// see `WatchItemsModel`.
struct OutOfReachTests {

    private let list = List(id: 1, uuid: "list-1", name: "Shop", ownerID: 9, role: .editor)

    private func item(_ id: Int64, _ name: String) -> Item {
        Item(id: id, uuid: "item-\(id)", name: name, amount: 1,
             unitID: nil, doneAt: nil, tagIDs: [])
    }

    /// Somewhere that cannot be reached: a phone in another room, a server behind a
    /// tunnel. The difference does not matter, which is the point of the protocol.
    private struct OutOfReach: Destination {
        func sync(_ operations: [SyncOperation]) async throws -> [AppliedOperation] {
            throw APIError.transport(NoServer())
        }
    }

    /// Back in reach, and accepting everything.
    private final class BackInReach: Destination, @unchecked Sendable {
        private(set) var heard: [SyncOperation] = []

        func sync(_ operations: [SyncOperation]) async throws -> [AppliedOperation] {
            heard += operations
            return operations.map { AppliedOperation(id: $0.id, outcome: "applied") }
        }
    }

    /// Still completely usable: a tick is taken, written down, and kept.
    @Test("out of reach, a change is still made and still kept")
    func aChangeSurvivesBeingOutOfReach() async {
        let cache = Cache.inMemory(sending: { true })
        cache.remember(lists: [list])
        cache.remember(items: [item(1, "Milk")], on: list)

        cache.outbox.setDone(item(1, "Milk"), on: list, done: true)
        cache.remember(items: [item(1, "Milk").withDone(true)], on: list)

        let drained = await cache.outbox.drain(through: OutOfReach())

        #expect(drained.sent == 0, "something reached a place that cannot be reached")
        #expect(cache.outbox.waiting == 1, "the change was thrown away rather than kept")
        #expect(
            cache.items(on: list).first?.isDone == true,
            "the tick did not survive on the device that made it"
        )
    }

    /// And it outlives the app, which is what "kept" has to mean: a watch is put down
    /// mid-shop and the app is gone by the car park.
    @Test("what was made out of reach survives the app being killed")
    func aChangeSurvivesTheProcess() async {
        let path = NSTemporaryDirectory() + "reach-\(UUID().uuidString).sqlite"
        defer { try? FileManager.default.removeItem(atPath: path) }

        let first = Cache(path: path, sending: { true })
        first.remember(lists: [list])
        first.outbox.setDone(item(1, "Milk"), on: list, done: true)
        _ = await first.outbox.drain(through: OutOfReach())

        let again = Cache(path: path, sending: { true })

        #expect(again.outbox.waiting == 1, "the change did not outlive the app")
    }

    /// The other half: when the far end comes back, everything waiting goes -- in the
    /// order it happened, because a tick and an untick are not commutative.
    @Test("back in reach, everything waiting goes, in order")
    func everythingGoesWhenReachAble() async {
        let cache = Cache.inMemory(sending: { true })
        cache.remember(lists: [list])

        cache.outbox.setDone(item(1, "Milk"), on: list, done: true)
        cache.outbox.setDone(item(2, "Bread"), on: list, done: true)
        cache.outbox.setDone(item(1, "Milk"), on: list, done: false)
        _ = await cache.outbox.drain(through: OutOfReach())
        #expect(cache.outbox.waiting == 3, "three changes were not all kept")

        let phone = BackInReach()
        let drained = await cache.outbox.drain(through: phone)

        #expect(drained.sent == 3, "not everything was sent when the far end came back")
        #expect(cache.outbox.waiting == 0, "the queue was not emptied")
        #expect(
            phone.heard.map(\.item) == ["item-1", "item-2", "item-1"],
            "the changes arrived out of order: \(phone.heard.map(\.item))"
        )
        #expect(
            phone.heard.last?.done == false,
            "the last word on Milk was the tick rather than the untick"
        )
    }

    /// Nothing is sent twice. A drain that succeeded and a device that comes back into
    /// reach again must not replay what has already landed.
    @Test("what has landed is not sent again")
    func nothingIsSentTwice() async {
        let cache = Cache.inMemory(sending: { true })
        cache.remember(lists: [list])
        cache.outbox.setDone(item(1, "Milk"), on: list, done: true)

        let phone = BackInReach()
        _ = await cache.outbox.drain(through: phone)
        _ = await cache.outbox.drain(through: phone)

        #expect(phone.heard.count == 1, "a change that had landed was sent again")
    }
}
