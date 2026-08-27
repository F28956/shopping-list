import Foundation

/// The words the phone and the watch use to talk to each other.
///
/// Shared rather than written out on both sides. The two halves live in different
/// targets and nothing links them together, so a mistyped key would not fail to
/// build — it would fail to answer, on a watch, in a shop.
///
/// ## The watch reads the phone, not the server
///
/// It used to ask the phone for a credential and then talk to the server itself. That
/// stopped working the day a server stopped being required: with none configured there
/// was nothing to hand over and nothing to talk to, so the watch app was simply dead —
/// and that is now the **default** state of a fresh install.
///
/// So the phone is the watch's server. It holds the cache, the queue and whatever
/// account there is; the watch holds a picture of a list and a way to tick it off.
/// Everything the watch would have needed to do this itself — a database, a keychain,
/// a token, an outbox, the units table — is gone, because the phone already has one of
/// each and WatchConnectivity keeps the two ends fed:
///
/// * **`updateApplicationContext`** carries the snapshot. Latest-wins, delivered while
///   both apps are in the background, and **persisted by the system** — so a watch that
///   has heard once shows the list instantly at the next launch with nothing running.
///   That persistence is why the watch needs no database of its own.
/// * **`transferUserInfo`** carries the ticks back. Queued, in order, retried by the
///   system until the phone takes them, and **also persisted** — so a tick made in a
///   shop with the phone in a locker is not lost. That queue is why the watch needs no
///   outbox of its own.
///
/// **What this costs:** a watch out of range of its phone can no longer reach the
/// server on its own. It still *shows* the last list it was given and still takes
/// ticks — they leave when the phone is next in range. Only a cellular watch genuinely
/// away from its phone loses anything, and the alternative was a second full client,
/// with its own cache, queue and merge rules, that does not work at all in the
/// configuration most people will be in.
enum WatchLink {
    /// The key both payloads travel under.
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

        /// Whether a server is involved at all, so the watch's status dot can say the
        /// same thing the phone's does rather than inventing its own vocabulary.
        var onDeviceOnly = false
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
        /// Already spelled, e.g. `2 kg`, or nil when there is nothing to say. Resolved
        /// on the phone so the watch carries no units table and no formatting rule.
        var measure: String?
        var done: Bool
        var tagIDs: [Int64] = []
    }

    struct TagOnTheWatch: Codable, Equatable, Hashable, Identifiable {
        var id: Int64
        var name: String
        var emoji: String?
    }

    /// One crossing-off, made on the wrist.
    ///
    /// Naturally idempotent, which is why there is no id and no record of what has been
    /// applied: setting a row done twice is setting it done. `at` is the watch's clock
    /// and travels with it, because the phone will queue this behind whatever else it
    /// holds and the ordering rules run on when it *happened* — docs/offline.md.
    struct Tick: Codable, Equatable, Versioned {
        var version = Snapshot.current
        var list: String
        var item: String
        var done: Bool
        var at: Date
    }

    /// The most items worth sending.
    ///
    /// An application context is capped by the system at a few hundred kilobytes, and
    /// a payload over it is refused outright — which would mean a watch showing nothing
    /// rather than a watch showing a lot. A cap here is the difference between "the
    /// long list is shortened" and "the list is gone", and the watch says which it is.
    static let cap = 400

    static func encode(_ snapshot: Snapshot) -> [String: Any] {
        guard let data = try? encoder.encode(snapshot) else { return [:] }
        return [payload: data]
    }

    static func encode(_ tick: Tick) -> [String: Any] {
        guard let data = try? encoder.encode(tick) else { return [:] }
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
