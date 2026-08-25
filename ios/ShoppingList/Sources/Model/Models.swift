import Foundation

/// The shapes the API returns.
///
/// Deliberately a subset: the phone shows lists and what is on them, so it decodes
/// those and ignores the rest. A field added to the API does not break this app, and
/// a field removed from these structs is a decision rather than an accident.
struct List: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
    let ownerID: Int64

    enum CodingKeys: String, CodingKey {
        case id, name
        case ownerID = "owner_id"
    }
}

struct Item: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
    let amount: Double
    let unitID: Int64?
    /// When it was ticked off, or nil while it is still needed. There is no separate
    /// flag, so the two cannot disagree.
    let doneAt: Date?

    var isDone: Bool { doneAt != nil }

    enum CodingKeys: String, CodingKey {
        case id, name, amount
        case unitID = "unit_id"
        case doneAt = "done_at"
    }
}

struct Unit: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
}

/// One page of rows, plus what a caller needs to walk the rest.
struct Page<T: Decodable>: Decodable {
    let items: [T]
    let total: Int64
    let hasMore: Bool

    enum CodingKeys: String, CodingKey {
        case items, total
        case hasMore = "has_more"
    }
}

extension Item {
    /// How much of it, or nothing at all.
    ///
    /// One of something unmeasured is the default and the commonest case, so printing
    /// "1" on most rows is noise dressed as information — the same rule the web UI
    /// follows, so the two do not disagree about what a row says.
    func measure(units: [Int64: String]) -> String? {
        let unit = unitID.flatMap { units[$0] }
        let quantity = amount == amount.rounded()
            ? String(Int(amount))
            : String(amount)

        switch (amount, unit) {
        case (1, nil): return nil
        case (_, nil): return quantity
        case (_, let unit?): return "\(quantity) \(unit)"
        }
    }
}
