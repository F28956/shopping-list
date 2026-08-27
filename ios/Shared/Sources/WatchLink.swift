import Foundation

/// The words the phone and the watch use to talk to each other.
///
/// Shared rather than written out on both sides. The two halves live in different
/// targets and nothing links them together, so a mistyped key would not fail to
/// build — it would fail to answer, on a watch, in a shop.
///
/// ## The watch is a client; the phone tells it which kind
///
/// A watch has to work with the phone left at home. So it keeps **its own cache and
/// its own outbox** — the same ones the phones use, the same rows, the same queue —
/// and it is a real client rather than a screen mirroring one. What changes between
/// the two modes is only where that queue drains to:
///
/// * **With a server**, the watch talks to it directly. It is completely independent:
///   its own cache, its own queue, its own requests. A watch with no network of its
///   own still reaches the server, because watchOS routes `URLSession` through the
///   paired phone when it has to — so this works on a non-cellular watch with the
///   phone merely nearby, and on a cellular one with the phone at home.
/// * **With no server**, the phone *is* the far end. It holds the only copy of the
///   lists there is, so it pushes what it has and accepts the watch's queue in
///   exactly the shape the server would have accepted it.
///
/// The queue drains through ``Destination`` either way, which is what keeps this from
/// being two clients: the rules about what to forget, what to keep, and what to say
/// out loud are written once and run in both modes.
///
/// **Config always comes from the phone, never from the wrist.** Nobody types a URL on
/// a watch and nobody signs in on one — there is no browser to run the flow in. So the
/// address arrives in the application context and the credential is asked for when
/// needed, and a watch that has never met its phone simply says so.
enum WatchLink {
    /// Watch asks for a credential with this key; the phone replies under the same one.
    ///
    /// Not in the application context with everything else, deliberately: a context is
    /// persisted and latest-wins, which is the wrong shape for a credential. It is
    /// asked for when it is needed and never stored anywhere it would go stale.
    static let tokenRequest = "token"

    /// The phone's answer to a token request also carries which server it is for.
    ///
    /// In the same message rather than a second one, because the two are useless
    /// apart: a watch holding a token for a server it cannot name would send it
    /// somewhere, and a watch that knew the address without a token could not use it.
    static let serverAddress = "server"

    /// The key the other payloads travel under.
    ///
    /// One key holding encoded JSON rather than a dictionary of loose keys: the shape
    /// is then described in one place, by types both targets compile, and adding a
    /// field is not a fifth place to remember to change.
    static let payload = "payload"

    /// What the phone knows, as much of it as is worth sending.
    ///
    /// Names rather than ids wherever the watch would otherwise need a lookup table —
    /// the unit is spelled out here so the watch needs no units of its own. The ids
    /// that remain are tag ids, which only ever travel back as part of ordering.
    struct Snapshot: Codable, Equatable, Versioned {
        /// What this payload's shape is. A watch updated before its phone, or after
        /// it, must not decode a message it does not understand into something
        /// plausible and wrong — so an unknown version is ignored rather than guessed.
        var version = Self.current
        static let current = 1

        var lists: [ListOnTheWatch] = []

        /// Whether a server is involved at all. This is what picks the watch's mode,
        /// and it is the phone's answer rather than the watch's guess — there is one
        /// place that decides whether this household has a server, and it is not here.
        var onDeviceOnly = false

        /// Where that server is, when there is one. The only way an address can reach
        /// a watch: nobody types a URL on a wrist.
        var server: String?

        /// The units, so the watch can spell a measure itself.
        ///
        /// Sent rather than pre-formatted on the phone. A spelled string looked
        /// simpler and made the watch's rows a *different shape* from the ones it
        /// holds with a server, which meant two ways to draw a row and one of them
        /// only exercised in one mode. With the ids the watch's cache holds the same
        /// `Item` either way and `measure(units:)` is the only formatter there is.
        var units: [UnitOnTheWatch] = []
    }

    struct UnitOnTheWatch: Codable, Equatable, Hashable, Identifiable {
        var id: Int64
        var name: String
    }

    /// The watch's queue, on its way to the phone.
    ///
    /// The server's own batch, unchanged — see ``Destination``. The phone applies it
    /// against the only copy of the lists there is and answers per operation, which is
    /// what lets the watch run the same drain in both modes.
    struct SyncRequest: Codable, Versioned {
        var version = Snapshot.current
        var operations: [SyncOperation]
    }

    struct SyncReply: Codable, Versioned {
        var version = Snapshot.current
        var outcomes: [Outcome]
    }

    /// What became of one queued operation.
    ///
    /// Deliberately smaller than the server's answer. That one carries the row it
    /// produced, so a device can learn the id of something it made offline; the watch
    /// only ever queues a crossing-off, which creates nothing and needs nothing back.
    /// Sending an item here would be sending something nobody reads.
    struct Outcome: Codable {
        var id: String
        /// `applied`, `already_applied` or `refused` — the server's words, so the
        /// drain does not need to learn a second vocabulary.
        var outcome: String
        var why: String?
    }

    struct ListOnTheWatch: Codable, Equatable, Hashable, Identifiable {
        /// The uuid, which is what a tick names. Not the server's `id`: on a device
        /// with no server there isn't one, and the uuid is the name that exists
        /// everywhere and never changes.
        var id: String
        var name: String
        /// In the order this list is walked, so the watch groups exactly as the phone
        /// does without owning the rule.
        var tags: [TagOnTheWatch] = []
        var items: [ItemOnTheWatch] = []
        /// How many there really are, when `items` is a prefix — see `Snapshot.cap`.
        var total: Int
        var truncated: Bool
    }

    struct ItemOnTheWatch: Codable, Equatable, Hashable, Identifiable {
        var id: String
        var name: String
        var amount: Double
        var unitID: Int64?
        var done: Bool
        var tagIDs: [Int64] = []
    }

    struct TagOnTheWatch: Codable, Equatable, Hashable, Identifiable {
        var id: Int64
        var name: String
        var emoji: String?
        /// Where this falls when a list is grouped. Carried rather than re-derived so
        /// the watch walks the shop in the same order every other screen does.
        var sortOrder: Int64
    }

    /// The most items worth sending.
    ///
    /// An application context is capped by the system at a few hundred kilobytes, and
    /// a payload over it is refused outright — which would mean a watch showing nothing
    /// rather than a watch showing a lot. A cap here is the difference between "the
    /// long list is shortened" and "the list is gone", and the watch says which it is.
    static let cap = 400

    static func encode<T: Codable & Versioned>(_ value: T) -> [String: Any] {
        guard let data = try? encoder.encode(value) else { return [:] }
        return [payload: data]
    }

    /// Reads a payload, or nothing.
    ///
    /// A version this build does not know is nothing, deliberately: see
    /// `Snapshot.version`.
    static func decode<T: Codable & Versioned>(_ message: [String: Any], as: T.Type) -> T? {
        guard let data = message[payload] as? Data,
              let decoded = try? decoder.decode(T.self, from: data),
              decoded.version == Snapshot.current
        else { return nil }
        return decoded
    }

    /// Something that says which shape it is, so an old build can refuse a new message
    /// rather than decode half of it. Both payloads carry it and both are checked.
    protocol Versioned {
        var version: Int { get }
    }

    private static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()

    private static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}
