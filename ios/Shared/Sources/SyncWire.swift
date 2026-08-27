import Foundation

/// The shapes `POST /api/sync` speaks.
///
/// Everything names rows by **uuid**, never by id. That is the whole reason the column
/// exists: an item added with no signal has no id, and will not have one until this
/// route answers — but it has been called by this uuid since the moment somebody typed
/// it, and every operation queued behind it says the same.
struct SyncBatch: Encodable {
    var operations: [SyncOperation]
}

struct SyncOperation: Encodable {
    /// What this operation is called. The server records it, so a resend is a no-op.
    var id: String
    /// When this device says it happened. The server clamps it forward only: behind is
    /// believed, ahead is not.
    var at: Date
    /// The list, by uuid.
    var list: String
    var kind: String
    var item: String?
    var items: [String]?
    var line: String?
    var name: String?
    var amount: Double?
    var unitID: Int64?
    /// The row as this device saw it, for an edit made against a copy. What decides
    /// between renaming a row and splitting one.
    var seen: SeenOn?
    var done: Bool?
    /// The aisle an `attach_tag` or `detach_tag` names.
    var tagID: Int64?

    enum CodingKeys: String, CodingKey {
        case id, at, list, kind, item, items, line, name, amount, seen, done
        case unitID = "unit_id"
        case tagID = "tag_id"
    }
}

struct SeenOn: Encodable {
    var name: String
    var amount: Double
    var unitID: Int64?

    enum CodingKeys: String, CodingKey {
        case name, amount
        case unitID = "unit_id"
    }
}

struct Replayed: Decodable {
    var operations: [AppliedOperation]
}

/// What became of one operation.
///
/// `outcome` is `applied`, `already_applied` or `refused`. A refusal carries `why`, and
/// every one of them is a sentence an app can put in front of somebody.
struct AppliedOperation: Decodable {
    var id: String
    var outcome: String
    var item: Item?
    /// The list a `make_list` produced. Absent on every other operation — a device
    /// that made a list offline knows what it called it and not what the server does,
    /// and this is where it finds out.
    var list: List?
    var why: String?

    var landed: Bool { outcome == "applied" || outcome == "already_applied" }

    /// Whether the device should keep this operation rather than forget it.
    ///
    /// Only for work refused because the person is no longer allowed on the list. That
    /// is the one refusal that may un-refuse itself: if they are invited back it is
    /// still here to send, and nothing was quietly binned behind them — see
    /// `docs/offline.md` (8). Everything else will be refused forever.
    var keepForLater: Bool { outcome == "refused" && why == "not_allowed" }

    /// What to tell somebody who watched themselves do this.
    var lost: String? {
        guard outcome == "refused" else { return nil }
        switch why {
        case "gone": return "Someone had already deleted it."
        case "list_gone": return "That list has been deleted."
        case "not_allowed": return "You are no longer on that list."
        default: return "The server would not accept it."
        }
    }
}
