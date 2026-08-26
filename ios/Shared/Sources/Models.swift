import Foundation

extension Double {
    /// "2" rather than "2.0", "1.5" left as it is.
    ///
    /// Counts are whole far more often than not, and a trailing ".0" reads as a
    /// measurement rather than a count. The magnitude guard is not decoration:
    /// `Int(_:)` traps on NaN, on infinity, and on anything past `Int.max`, and the
    /// amount arrives from the network.
    var asAmount: String {
        guard self == rounded(), abs(self) < 1e15 else { return String(self) }
        return String(Int(self))
    }
}

/// The shapes the API returns.
///
/// Deliberately a subset: the phone shows lists and what is on them, so it decodes
/// those and ignores the rest. A field added to the API does not break this app, and
/// a field removed from these structs is a decision rather than an accident.
/// What this person may do with a list.
///
/// Ordered, so a needed role can be compared against the held one — `held >= .editor`
/// reads the way the service's own checks do. Decoded from the server rather than
/// guessed from `ownerID`: a list shared as editor is not owned and is not read-only.
enum Role: String, Decodable, Comparable {
    case viewer, editor, owner

    private var rank: Int {
        switch self {
        case .viewer: return 0
        case .editor: return 1
        case .owner: return 2
        }
    }

    static func < (a: Role, b: Role) -> Bool { a.rank < b.rank }
}

struct List: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
    let ownerID: Int64
    let role: Role

    var mayEdit: Bool { role >= .editor }

    enum CodingKeys: String, CodingKey {
        case id, name, role
        case ownerID = "owner_id"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        ownerID = try c.decode(Int64.self, forKey: .ownerID)
        // The safe reading of a server that did not say: a list you may not change is
        // a list shown read-only, not one that offers controls and then refuses them.
        role = try c.decodeIfPresent(Role.self, forKey: .role) ?? .viewer
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
    /// What this is filed under, in the order the shop is walked. Empty on the
    /// routes that answer with one item: only the list route joins them, because
    /// only a list needs grouping.
    let tagIDs: [Int64]

    var isDone: Bool { doneAt != nil }

    enum CodingKeys: String, CodingKey {
        case id, name, amount
        case unitID = "unit_id"
        case doneAt = "done_at"
        case tagIDs = "tag_ids"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        amount = try c.decode(Double.self, forKey: .amount)
        unitID = try c.decodeIfPresent(Int64.self, forKey: .unitID)
        doneAt = try c.decodeIfPresent(Date.self, forKey: .doneAt)
        // Absent rather than empty on the single-item routes, and absent is not a
        // decoding failure: an item that does not say what it is filed under is not
        // a broken item.
        tagIDs = try c.decodeIfPresent([Int64].self, forKey: .tagIDs) ?? []
    }
}

/// Somebody who can see a list, and what they may do with it.
struct Person: Identifiable, Decodable, Hashable {
    let userID: Int64
    let name: String?
    let email: String?
    let role: Role

    var id: Int64 { userID }

    /// What to call them. A name if the provider gave one, else the address, else
    /// something honest — an account can have neither, and "Someone" at least does
    /// not pretend otherwise.
    var shown: String { name ?? email ?? "Someone" }

    enum CodingKeys: String, CodingKey {
        case name, email, role
        case userID = "user_id"
    }
}

/// The signed-in person, as the server knows them.
struct Me: Decodable {
    let id: Int64
}

struct Unit: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
}

struct Tag: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
    let emoji: String?
    /// Where this falls when a list is grouped: the order of the shop, not the
    /// alphabet. Sorting by it is using what the server sends, not second-guessing
    /// it -- the field exists precisely so every client agrees on the order.
    let sortOrder: Int64

    enum CodingKeys: String, CodingKey {
        case id, name, emoji
        case sortOrder = "sort_order"
    }
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

/// Rows, and whether they are all of them.
///
/// `truncated` exists to be shown. The browser says "Showing 12 of 340" when a list
/// outruns a page, because a prefix presented as the whole thing makes the missing
/// rows look deleted rather than merely elsewhere — and these apps were quietly doing
/// exactly that, having decoded `has_more` and never read it.
struct Listing<T> {
    let items: [T]
    let total: Int64
    let truncated: Bool

    init(_ page: Page<T>) where T: Decodable {
        items = page.items
        total = page.total
        truncated = page.hasMore
    }

    init(items: [T], total: Int64, truncated: Bool) {
        self.items = items
        self.total = total
        self.truncated = truncated
    }

    func map<U>(_ transform: ([T]) -> [U]) -> Listing<U> {
        Listing<U>(items: transform(items), total: total, truncated: truncated)
    }
}

extension Item {
    /// How much of it, or nothing at all.
    ///
    /// One of something unmeasured is the default and the commonest case, so printing
    /// "1" on most rows is noise dressed as information — the same rule the web UI
    /// follows, so the two do not disagree about what a row says.
    func measure(units: [Int64: String]) -> String? {
        // `unit` is the unit that means "counted, not measured", and it is what an
        // item added without one is given. It says nothing a number does not, so it
        // prints as nothing: six eggs, not "6 unit".
        let unit = unitID.flatMap { units[$0] }.flatMap { $0 == "unit" ? nil : $0 }
        // Through `asAmount` rather than repeating its rule: this had its own copy,
        // without the guard that keeps `Int(_:)` from trapping on a value off the
        // end of the number line.
        let quantity = amount.asAmount

        switch (amount, unit) {
        case (1, nil): return nil
        case (_, nil): return quantity
        case (_, let unit?): return "\(quantity) \(unit)"
        }
    }
}
