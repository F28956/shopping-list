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
    /// What operations call this list, minted wherever it was made.
    ///
    /// `id` is the server's counter; this is the name a queued change uses, because a
    /// device with no signal has one of these and not the other. Empty when it came
    /// from a server that predates the column, which is not worth failing a decode
    /// over — nothing queues against it yet.
    let uuid: String
    let name: String
    let ownerID: Int64
    let role: Role

    var mayEdit: Bool { role >= .editor }

    init(id: Int64, uuid: String, name: String, ownerID: Int64, role: Role) {
        self.id = id
        self.uuid = uuid
        self.name = name
        self.ownerID = ownerID
        self.role = role
    }

    enum CodingKeys: String, CodingKey {
        case id, uuid, name, role
        case ownerID = "owner_id"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        uuid = try c.decodeIfPresent(String.self, forKey: .uuid) ?? ""
        name = try c.decode(String.self, forKey: .name)
        ownerID = try c.decode(Int64.self, forKey: .ownerID)
        // The safe reading of a server that did not say: a list you may not change is
        // a list shown read-only, not one that offers controls and then refuses them.
        role = try c.decodeIfPresent(Role.self, forKey: .role) ?? .viewer
    }
}

struct Item: Identifiable, Decodable, Hashable {
    let id: Int64
    /// What operations call this item. See [`List.uuid`].
    let uuid: String
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

    /// The same item, ticked off or put back, without asking anybody.
    ///
    /// For the optimistic half of an offline edit: the row moves on screen now, and
    /// the queue carries the promise that the server will hear about it. The stamp is
    /// this device's clock, and it is replaced by the server's the moment a real answer
    /// arrives.
    func withDone(_ done: Bool) -> Item {
        Item(
            id: id,
            uuid: uuid,
            name: name,
            amount: amount,
            unitID: unitID,
            doneAt: done ? Date() : nil,
            tagIDs: tagIDs
        )
    }

    init(
        id: Int64,
        uuid: String,
        name: String,
        amount: Double,
        unitID: Int64?,
        doneAt: Date?,
        tagIDs: [Int64]
    ) {
        self.id = id
        self.uuid = uuid
        self.name = name
        self.amount = amount
        self.unitID = unitID
        self.doneAt = doneAt
        self.tagIDs = tagIDs
    }

    enum CodingKeys: String, CodingKey {
        case id, uuid, name, amount
        case unitID = "unit_id"
        case doneAt = "done_at"
        case tagIDs = "tag_ids"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        uuid = try c.decodeIfPresent(String.self, forKey: .uuid) ?? ""
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
    /// Whether this person administers *this server* — who may sign in, and who else
    /// administers it.
    ///
    /// A fact about the server rather than about them: the same account on somebody
    /// else's server is not an owner of it, which is why it arrives beside the person
    /// rather than on them. It is not a data role — an owner has no more access to
    /// anybody's lists than anybody else.
    let isOwner: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case isOwner = "is_owner"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        // Absent on a server older than this app, where nobody is an owner because the
        // idea did not exist. Defaulted rather than refused: the rest of `Me` is still
        // true and the screen it gates simply does not appear.
        isOwner = try c.decodeIfPresent(Bool.self, forKey: .isOwner) ?? false
    }
}

/// One address that may sign in to this server.
struct Admitted: Identifiable, Decodable, Hashable {
    let email: String
    /// Who it turned out to be, once they signed in. `nil` means nobody has used this
    /// address yet — the difference between "invited" and "here".
    let userID: Int64?
    let note: String?

    var id: String { email }

    /// Whether anybody has used it. The screen says so, because withdrawing an address
    /// somebody is using signs them out and withdrawing one nobody has used does not.
    var isInUse: Bool { userID != nil }

    enum CodingKeys: String, CodingKey {
        case email, note
        case userID = "user_id"
    }
}

/// What a server says about itself, over the wire. Mirrors `ServerDirectory.About`,
/// which is the same shape read before anybody has signed in.
struct ServerAbout: Decodable {
    let name: String
    let version: String
    /// `open`, `closed` or `unclaimed`.
    let admission: String

    var admitsAnyone: Bool { admission == "open" }
}

struct Unit: Identifiable, Decodable, Hashable {
    let id: Int64
    let name: String
    /// Whether this unit means something written with no number in front of it —
    /// `pint milk`. Not every unit may be: half of them are also the first word of
    /// ordinary things to buy, and `can opener` is not one can of opener. It is a fact
    /// about each unit rather than a rule here, so the server and this agree.
    ///
    /// Absent is `false`, and it has to be spelled out: Swift's synthesised decoder
    /// ignores a property's default value and throws on a missing key, so a server
    /// that predates this column would have failed to decode a single unit.
    let bare: Bool

    enum CodingKeys: String, CodingKey {
        case id, name, bare
    }

    init(id: Int64, name: String, bare: Bool = false) {
        self.id = id
        self.name = name
        self.bare = bare
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        bare = try c.decodeIfPresent(Bool.self, forKey: .bare) ?? false
    }
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
        // A unit is never hidden. `unit` — the one that means counted rather than
        // measured — used to print as nothing, on the grounds that it says nothing a
        // number does not. It turned out to say one thing that matters: that the row
        // has a unit at all. A row showing nothing was indistinguishable from a row
        // that had lost one, and the only way to tell was to look in the database.
        let unit = unitID.flatMap { units[$0] }
        // Through `asAmount` rather than repeating its rule: this had its own copy,
        // without the guard that keeps `Int(_:)` from trapping on a value off the
        // end of the number line.
        let quantity = amount.asAmount

        // Nothing at all is left for the rows that genuinely have no unit: those
        // predate the rule that gives every item one, and one of something unmeasured
        // is still a row where "1" would be noise dressed as information.
        switch (amount, unit) {
        case (1, nil): return nil
        case (_, nil): return quantity
        case (_, let unit?): return "\(quantity) \(unit)"
        }
    }
}

/// One line this list remembers, as the server keeps it.
///
/// The device stores these and resolves against them — see `QuickAdd.resolve`. What
/// makes that safe is that both ends are now reading the same memory: a phone with a
/// copy of this reaches the server's answer for the same words, rather than its own.
struct RememberedEntry: Decodable {
    /// The key: trimmed and lowercased.
    let name: String
    /// The spelling last used, for showing back.
    let display: String
    let unitID: Int64?
    let amount: Double?
    let tags: [Int64]
    let uses: Int64
    /// Unix seconds.
    let lastUsedAt: Int64

    enum CodingKeys: String, CodingKey {
        case name, display, amount, tags, uses
        case unitID = "unit_id"
        case lastUsedAt = "last_used_at"
    }
}

/// The one key a name is remembered under.
///
/// Trimmed and lowercased, so `Milk`, `milk ` and `MILK` are one memory.
///
/// Mirrors `parsing::add::fold`, and is spelled out here rather than called across the
/// boundary because the watch links the store without linking the parser. What is
/// being avoided is not a mirror — it is the *second* mirror: the store folded one way
/// and the lookup another, so a name with a trailing space was written under one key
/// and looked for under a different one. One definition, used by both.
func foldedName(_ name: String) -> String {
    name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
}
