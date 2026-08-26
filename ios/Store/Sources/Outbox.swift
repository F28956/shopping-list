import Foundation
import GRDB

/// One change made on this device that the server has not been told about yet.
struct QueuedOperation: Identifiable, Equatable {
    /// This device's order. The row id, so it can only ever count up.
    var sequence: Int64
    /// What this operation is called, everywhere. Minted here, sent as-is.
    var id: String
    var kind: String
    var listID: Int64
    var listUUID: String
    var itemID: Int64
    /// What the item is called once operations name rows rather than ids — see
    /// `docs/offline.md`. Carried from the first day so the table needs no migration
    /// when `POST /api/sync` lands.
    var itemUUID: String
    /// The arguments, as JSON. `{"done":true}` for ``Kind/setDone``.
    var payload: String
    /// When this device says it happened. Not sent yet: the REST routes stamp their
    /// own time, and carrying this is what `POST /api/sync` is for. Recorded anyway,
    /// so the queue is not lying about when it was written.
    var at: Date

    enum Kind {
        static let setDone = "set_done"
    }

    /// Whether a queued ``Kind/setDone`` is a tick or an untick.
    var done: Bool { payload.contains("\"done\":true") }

    /// What to call this when telling somebody it could not be applied.
    var described: String {
        switch kind {
        case Kind.setDone: return done ? "crossing something off" : "putting something back"
        default: return "a change"
        }
    }
}

/// What happened when the queue was last drained.
///
/// `sent` is how many reached the server, `waiting` how many are still here, and
/// `dropped` names the ones that will never land — an item somebody deleted while this
/// device was away. Dropped work is worth telling somebody about: they watched
/// themselves do it. Refused work is *not* dropped; see ``Outbox/drain(through:)``.
struct Drained: Equatable {
    var sent: Int = 0
    var waiting: Int = 0
    var dropped: [String] = []
}

/// Changes made on this device that the server has not been told about yet.
///
/// The counterpart of ``Cache``: that one holds what the server said, this one holds
/// what this device said back. Together they are what lets somebody cross things off
/// in a shop with no signal and find the list right when they come out.
///
/// Unlike the cache, **this is not disposable**. A queued change exists nowhere else in
/// the world until it is sent, which is why the database it shares with the cache is
/// migrated by hand rather than thrown away on a schema change.
///
/// Ordering is `sequence`, which is the row id and therefore monotonic: a device's own
/// changes replay in the order they were made, always. Ordering *between* devices is a
/// different question, decided by `at` — see `docs/offline.md`.
final class Outbox: @unchecked Sendable {

    private let queue: DatabaseQueue?

    init(queue: DatabaseQueue?) {
        self.queue = queue
    }

    /// Queues a tick.
    ///
    /// The caller has already changed what is on screen. This is the promise that the
    /// change will reach the server eventually, and the only place it exists until it
    /// does.
    func setDone(_ item: Item, on list: List, done: Bool) {
        write { db in
            try db.execute(
                sql: """
                INSERT INTO operations
                    (id, kind, list_id, list_uuid, item_id, item_uuid, payload, at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                arguments: [
                    UUID().uuidString.lowercased(),
                    QueuedOperation.Kind.setDone,
                    list.id,
                    list.uuid,
                    item.id,
                    item.uuid,
                    done ? "{\"done\":true}" : "{\"done\":false}",
                    Date(),
                ]
            )
        }
    }

    /// Everything queued, oldest first.
    func all() -> [QueuedOperation] {
        rows(sql: "SELECT * FROM operations ORDER BY sequence", arguments: [])
    }

    /// What is queued against one list, oldest first.
    func forList(_ list: List) -> [QueuedOperation] {
        rows(
            sql: "SELECT * FROM operations WHERE list_id = ? ORDER BY sequence",
            arguments: [list.id]
        )
    }

    var waiting: Int { all().count }

    /// Called when somebody signs out.
    ///
    /// The queue goes too. Its contents are changes to somebody else's lists, made by
    /// somebody who is no longer here, and sending them under the next person's token
    /// would be a stranger writing to a stranger's shopping.
    func forgetEverything() {
        write { db in try db.execute(sql: "DELETE FROM operations") }
    }

    /// Sends what is queued, oldest first, and stops at the first thing it cannot send.
    ///
    /// **In order, and stopping.** The queue is this device's story of what happened,
    /// and skipping past a stuck operation to send a later one would tell that story
    /// out of order — ticking something off after a delete that has not gone yet.
    ///
    /// What each outcome means:
    ///
    /// - **Sent** — forgotten. The server has it.
    /// - **No connection** — kept, and the drain stops. The ordinary case, and not an
    ///   error.
    /// - **The row is gone** — dropped, and named in the result. Delete is final (see
    ///   `docs/offline.md`): a tick has nothing to land on and never will. The person
    ///   is told, because they watched themselves tick it.
    /// - **Refused** — kept, and the drain stops. Somebody removed from a list keeps
    ///   their queue: if they are invited back the work is still there, and nothing was
    ///   quietly binned behind them.
    /// - **Anything else** — dropped. A malformed operation the server will refuse
    ///   forever would block everything behind it for good.
    func drain(through api: API) async -> Drained {
        var sent = 0
        var dropped: [String] = []

        for operation in all() {
            do {
                try await send(operation, through: api)
                forget(operation)
                sent += 1
            } catch let problem as APIError {
                switch problem {
                case .transport, .forbidden, .notAdmitted, .unauthorized:
                    return Drained(sent: sent, waiting: waiting, dropped: dropped)
                case .notFound:
                    forget(operation)
                    dropped.append(operation.described)
                case .badInput, .server:
                    forget(operation)
                    dropped.append(operation.described)
                }
            } catch {
                forget(operation)
                dropped.append(operation.described)
            }
        }

        return Drained(sent: sent, waiting: waiting, dropped: dropped)
    }

    private func send(_ operation: QueuedOperation, through api: API) async throws {
        switch operation.kind {
        case QueuedOperation.Kind.setDone:
            try await api.setDone(
                itemID: operation.itemID,
                listID: operation.listID,
                done: operation.done
            )
        default:
            // A kind this build does not know is a downgrade, which cannot arise yet.
            // Refused rather than skipped, so it is never silent.
            throw APIError.badInput("Unknown queued operation: \(operation.kind)")
        }
    }

    private func forget(_ operation: QueuedOperation) {
        write { db in
            try db.execute(
                sql: "DELETE FROM operations WHERE sequence = ?",
                arguments: [operation.sequence]
            )
        }
    }

    private func rows(sql: String, arguments: StatementArguments) -> [QueuedOperation] {
        guard let queue else { return [] }
        let found = try? queue.read { db in
            try Row.fetchAll(db, sql: sql, arguments: arguments).map { row in
                QueuedOperation(
                    sequence: row["sequence"],
                    id: row["id"],
                    kind: row["kind"],
                    listID: row["list_id"],
                    listUUID: row["list_uuid"],
                    itemID: row["item_id"],
                    itemUUID: row["item_uuid"],
                    payload: row["payload"],
                    at: row["at"]
                )
            }
        }
        return found ?? []
    }

    private func write(_ work: @escaping (Database) throws -> Void) {
        guard let queue else { return }
        try? queue.write(work)
    }
}
