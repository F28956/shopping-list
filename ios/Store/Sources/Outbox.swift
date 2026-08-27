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
    /// The row's id on the server, where there is one. Negative for a row this device
    /// made offline, and used only for marking the screen — what goes on the wire is
    /// always the uuid.
    var itemID: Int64
    /// What operations call the row. The only name that travels.
    var itemUUID: String
    /// The arguments, as JSON — whatever the kind needs beyond the columns beside it.
    var payload: String
    /// When this device says it happened. Sent with the operation and clamped forward
    /// by the server; behind is believed.
    var at: Date

    enum Kind {
        /// Make the list itself. Queued when a list is written down with nowhere to
        /// send it — no signal, or no server at all.
        static let makeList = "make_list"
        static let add = "add"
        static let setDone = "set_done"
        static let update = "update"
        static let delete = "delete"
        static let clearDone = "clear_done"
        static let attachTag = "attach_tag"
        static let detachTag = "detach_tag"
        static let setTagOrder = "set_tag_order"
    }

    // MARK: - Reading the payload
    //
    // The five kinds want different arguments, and five sets of nullable columns would
    // be worse than one blob. These are the readings the screen needs to lay unsent work
    // back over the server's answer — see `ItemsView.withUnsent`.

    private var fields: [String: Any] {
        (try? JSONSerialization.jsonObject(with: Data(payload.utf8))) as? [String: Any] ?? [:]
    }

    /// Whether a queued ``Kind/setDone`` is a tick or an untick.
    var done: Bool { fields["done"] as? Bool ?? false }
    /// The name a queued ``Kind/update`` gives the row.
    var editedName: String? { fields["name"] as? String }
    /// The amount a queued ``Kind/update`` gives the row.
    var editedAmount: Double? { fields["amount"] as? Double }
    /// The unit a queued ``Kind/update`` gives the row.
    var editedUnitID: Int64? { (fields["unit_id"] as? NSNumber)?.int64Value }
    /// The rows a queued ``Kind/clearDone`` named.
    var sweptUUIDs: Set<String> { Set(fields["items"] as? [String] ?? []) }
    /// The aisle a queued ``Kind/attachTag`` or ``Kind/detachTag`` names.
    var tagID: Int64? { (fields["tag_id"] as? NSNumber)?.int64Value }
    /// The walk a queued ``Kind/setTagOrder`` describes.
    var tagIDs: [Int64]? { (fields["tag_ids"] as? [NSNumber])?.map(\.int64Value) }

    /// This operation as the route wants it.
    var onTheWire: SyncOperation {
        let seen = fields["seen"] as? [String: Any]
        return SyncOperation(
            id: id,
            at: at,
            list: listUUID,
            kind: kind,
            item: itemUUID.isEmpty ? nil : itemUUID,
            items: fields["items"] as? [String],
            line: fields["line"] as? String,
            name: editedName,
            amount: editedAmount,
            unitID: editedUnitID,
            seen: seen.map {
                SeenOn(
                    name: $0["name"] as? String ?? "",
                    amount: $0["amount"] as? Double ?? 1,
                    unitID: ($0["unit_id"] as? NSNumber)?.int64Value
                )
            },
            done: kind == Kind.setDone ? done : nil,
            tagID: tagID,
            tagIDs: tagIDs
        )
    }
}

/// A list this device made offline, and the row the server made for it.
struct Adopted: Equatable {
    /// The name that never changed, and the only one both ends agree on.
    var uuid: String
    var real: List
}

/// What happened when the queue was last drained.
///
/// `sent` reached the server. `waiting` is still here — either because there was no
/// connection, or because it was refused for want of access and is being kept in case
/// that changes. `lost` names what will never land, in words for a person: they watched
/// themselves do it, so it is worth saying.
struct Drained: Equatable {
    var sent: Int = 0
    var waiting: Int = 0
    var lost: [String] = []
    /// Lists this device made offline, and the rows the server made for them, paired
    /// by the `uuid` that never changed. The caller swaps this device's own numbering
    /// for the server's — see `Cache.adopt`.
    var adopted: [Adopted] = []
    /// Something was refused. The one state of the three that interrupts.
    var refused: Bool = false
}

/// Changes made on this device that the server has not been told about yet.
///
/// The counterpart of ``Cache``: that one holds what the server said, this one holds
/// what this device said back. Together they are what lets somebody shop with no signal
/// and find the list right when they come out.
///
/// Unlike the cache, **this is not disposable**. A queued change exists nowhere else in
/// the world until it is sent, which is why the database it shares with the cache is
/// migrated by hand rather than thrown away on a schema change.
final class Outbox: @unchecked Sendable {

    private let queue: DatabaseQueue?

    init(queue: DatabaseQueue?) {
        self.queue = queue
    }

    // MARK: - Queueing
    //
    // Every one of these is called after the screen has already changed. They are the
    // promise that the change will reach the server eventually, and the only place it
    // exists until it does.

    /// Says a list exists, under the name this device has been calling it by.
    ///
    /// Names no item, which is why `itemUUID` is empty — the wire drops it, and the
    /// list's own `uuid` is the only name this operation needs.
    func makeList(_ list: List) {
        queue(Kind.makeList, "", list.id, list, ["name": list.name])
    }

    /// Puts something on the list, under a name this device mints now.
    /// Puts something on the list, as this device has already read it.
    ///
    /// The **resolved** fields rather than the line somebody typed. Both are accepted
    /// by the sync route, and this used to send the line and let the server work it
    /// out again -- which meant the same words were read twice, once here to draw the
    /// row and once there to store it, and the two could reach different answers from
    /// different memories. They cannot now: the history is the list's and this device
    /// has a copy, so both ends read the same words against the same memory. Sending
    /// what was already decided is the shorter way to say that.
    ///
    /// Tags are not here -- the route has no field for them, and the caller queues an
    /// `attach_tag` behind this instead. The queue is ordered, so it lands after.
    func add(
        uuid: String,
        localID: Int64,
        name: String,
        amount: Double,
        unitID: Int64?,
        on list: List
    ) {
        var fields: [String: Any] = ["name": name, "amount": amount]
        if let unitID { fields["unit_id"] = unitID }
        queue(Kind.add, uuid, localID, list, fields)
    }

    /// Crosses something off, or puts it back.
    ///
    /// `at` is when it *happened*, which is not always now: a tick made on a watch
    /// reaches the phone whenever the two are next in range, and the server's ordering
    /// rules run on when somebody decided rather than on when the news arrived. See
    /// `docs/offline.md`.
    func setDone(_ item: Item, on list: List, done: Bool, at: Date = Date()) {
        queue(Kind.setDone, item.uuid, item.id, list, ["done": done], at: at)
    }

    /// Corrects what somebody typed, carrying what the row looked like at the time.
    ///
    /// `seen` is not decoration: it is what lets the server tell a plain rename from a
    /// rename of something somebody else has edited meanwhile, and split rather than
    /// overwrite. See `docs/offline.md` (5).
    func update(
        _ item: Item,
        on list: List,
        name: String,
        amount: Double,
        unitID: Int64?
    ) {
        var seen: [String: Any] = ["name": item.name, "amount": item.amount]
        if let was = item.unitID { seen["unit_id"] = was }

        var fields: [String: Any] = ["name": name, "amount": amount, "seen": seen]
        if let unitID { fields["unit_id"] = unitID }

        queue(Kind.update, item.uuid, item.id, list, fields)
    }

    /// Files something under an aisle, or stops filing it there.
    ///
    /// The tag travels as an id, and that is only safe because the ids are agreed in
    /// advance: `reference.json` is the same file the server's seed is checked against,
    /// so a device that has never met a server still means aisle 5 by 5.
    func tag(_ item: Item, on list: List, tagID: Int64, attached: Bool) {
        queue(
            attached ? Kind.attachTag : Kind.detachTag,
            item.uuid,
            item.id,
            list,
            ["tag_id": tagID]
        )
    }

    /// Records the order this person walks this list in.
    ///
    /// About a list rather than a row, so there is no item to name -- the same shape as
    /// a sweep. Per person on the server, which is what makes last-write-wins safe: it
    /// cannot overwrite anybody else's walk.
    func setTagOrder(_ tags: [Tag], on list: List) {
        queue(Kind.setTagOrder, "", 0, list, ["tag_ids": tags.map(\.id)])
    }

    func delete(_ item: Item, on list: List) {
        queue(Kind.delete, item.uuid, item.id, list, [:])
    }

    /// Empties the trolley of exactly the rows this device could see.
    ///
    /// The ids are the point. "Clear everything that is done" replayed an hour later is
    /// a different sentence, and would sweep away what somebody else ticked off
    /// meanwhile — `docs/offline.md` (4).
    func clearDone(_ done: [Item], on list: List) {
        // A sweep is about a list, not a row, so there is no item to name. The column is
        // not nullable and an empty string is the honest value for "no row".
        queue(Kind.clearDone, "", 0, list, ["items": done.map(\.uuid)])
    }

    private typealias Kind = QueuedOperation.Kind

    private func queue(
        _ kind: String,
        _ itemUUID: String,
        _ itemID: Int64,
        _ list: List,
        _ fields: [String: Any],
        at: Date = Date()
    ) {
        let payload = (try? JSONSerialization.data(withJSONObject: fields))
            .flatMap { String(data: $0, encoding: .utf8) } ?? "{}"

        write { db in
            try db.execute(
                sql: """
                INSERT INTO operations
                    (id, kind, list_id, list_uuid, item_id, item_uuid, payload, at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                arguments: [
                    UUID().uuidString.lowercased(), kind, list.id, list.uuid,
                    itemID, itemUUID, payload, at,
                ]
            )
        }
    }

    // MARK: - Reading

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

    // MARK: - Sending

    /// Sends the whole queue in one request and acts on what comes back.
    ///
    /// One request rather than one per operation: the batch is this device's story of
    /// what it did, and the server replays it in order. Each operation gets its own
    /// answer, so a refusal costs that change and no other.
    ///
    /// What each answer means:
    ///
    /// - **Applied**, or applied on an earlier send — forgotten. The server has it.
    /// - **Refused because the list will not have you** — kept. If they are invited back
    ///   the work is still here, and nothing was binned behind them. Reported, because
    ///   this is the state worth interrupting somebody for.
    /// - **Refused for any other reason** — forgotten, and named. A row somebody deleted
    ///   is not coming back, and blocking the queue on it would cost every change behind
    ///   it too.
    /// - **No connection** — nothing is forgotten and nothing is said. The ordinary case.
    func drain(through destination: Destination) async -> Drained {
        let queued = all()
        guard !queued.isEmpty else { return Drained() }

        let answers: [AppliedOperation]
        do {
            answers = try await destination.sync(queued.map(\.onTheWire))
        } catch {
            // The request itself did not get through, or the route refused it rather
            // than the changes in it. Either way nothing here is thrown away.
            return Drained(waiting: queued.count)
        }

        var sent = 0
        var lost: [String] = []
        var refused = false
        var adopted: [Adopted] = []

        for answer in answers {
            if answer.landed {
                // A list this device made has just been given its real id. Collected
                // rather than applied here, because the cache is the caller's and this
                // type deliberately knows nothing about it.
                if let made = answer.list, let queued = queued.first(where: { $0.id == answer.id }) {
                    adopted.append(Adopted(uuid: queued.listUUID, real: made))
                }
                forget(answer.id)
                sent += 1
            } else if answer.keepForLater {
                refused = true
            } else {
                forget(answer.id)
                if let said = answer.lost { lost.append(said) }
            }
        }

        return Drained(sent: sent, waiting: waiting, lost: lost, adopted: adopted, refused: refused)
    }

    private func forget(_ id: String) {
        write { db in
            try db.execute(sql: "DELETE FROM operations WHERE id = ?", arguments: [id])
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

/// Somewhere a queue can be emptied to.
///
/// One method, and it is the sync route's: a batch of operations in, an answer for each
/// one out. Everything the drain does with those answers — what to forget, what to keep
/// for later, what to say out loud — is the same wherever they came from, which is why
/// this is a protocol rather than two drains.
///
/// Two things conform. `API`, which is the server. And, on the watch, the **phone** —
/// because with no server the phone is where a queue goes, and it can answer the same
/// questions about a batch that a server can. The watch is then the same client in both
/// modes, holding the same cache and the same queue, and only the address of the far end
/// changes.
protocol Destination {
    func sync(_ operations: [SyncOperation]) async throws -> [AppliedOperation]
}

extension API: Destination {}
