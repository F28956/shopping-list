import Foundation
import GRDB

/// What was on the screen the last time the server answered.
///
/// This exists because of one bug: with no signal the app said there were no lists,
/// which is the app claiming an emptiness it never verified. A person who has lists is
/// told they have none. The only honest answers are "here is what I last saw" and
/// "I do not know" — never "there is nothing".
///
/// A cache, not yet a source of truth. Reads fall back to it when the server cannot be
/// reached; writes still go straight to the server and still fail when it cannot. The
/// outbox that changes that is step 3 of `docs/offline.md`, and these tables are shaped
/// so it can arrive beside them: rows are keyed on the server's `id` and carry the
/// `uuid` that queued operations will name them by.
///
/// Every method swallows its own errors. A cache that cannot be read is a cache that
/// holds nothing, and a screen asking for the last thing it saw has nothing useful to
/// do with a thrown error.
final class Cache: @unchecked Sendable {

    private let queue: DatabaseQueue?

    /// The queue of changes that have not been sent, in the same file — see ``Outbox``.
    let outbox: Outbox

    /// Opens the cache in Application Support, or gives up quietly.
    ///
    /// A `nil` queue is a working app with no memory rather than a crash on launch:
    /// the only thing this holds is a copy of what the server has, so a disk that
    /// will not cooperate costs a blank screen with no signal and nothing else.
    init(named name: String = "cache.sqlite") {
        queue = Self.open(named: name)
        outbox = Outbox(queue: queue)
        migrate()
    }

    /// A cache at an exact path, for tests that need one to outlive the object — the
    /// queue surviving the app being killed is the whole point of it, and that is only
    /// checkable by opening the same file twice.
    init(path: String) {
        queue = try? DatabaseQueue(path: path)
        outbox = Outbox(queue: queue)
        migrate()
    }

    /// An in-memory cache, for tests and for `-uiTesting`, which must not read or
    /// write whatever the person running it happens to have on disk.
    static func inMemory() -> Cache { Cache(named: "") }

    /// The one cache, because there is one database file.
    ///
    /// A static where this codebase otherwise passes its dependencies in, and that is
    /// the reason: `api` is per-signed-in-person and worth threading through, while
    /// this is a file on disk that two instances would fight over. Under `-uiTesting`
    /// it is in memory, so a test run neither reads nor overwrites whatever is
    /// actually cached on the machine running it.
    static let shared: Cache = {
        #if DEBUG
            if UITesting.isRunning { return .inMemory() }
        #endif
        return Cache()
    }()

    private static func open(named name: String) -> DatabaseQueue? {
        do {
            guard !name.isEmpty else { return try DatabaseQueue() }

            let folder = try FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            )
            .appendingPathComponent("ShoppingList", isDirectory: true)

            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            return try DatabaseQueue(path: folder.appendingPathComponent(name).path)
        } catch {
            return nil
        }
    }

    /// The schema, versioned the way the server's is.
    ///
    /// Migrated, never erased. The cached rows in here could be discarded on a schema
    /// change — they are a copy of what the server holds — but the outbox beside them
    /// holds changes that exist nowhere else, and the two share a file. So `v1` and
    /// everything after it are written by hand, and nobody loses a shop's worth of
    /// ticks to an app update.
    private func migrate() {
        guard let queue else { return }

        var migrator = DatabaseMigrator()

        migrator.registerMigration("v1") { db in
            try db.create(table: "lists") { t in
                t.column("id", .integer).primaryKey()
                t.column("uuid", .text).notNull()
                t.column("name", .text).notNull()
                t.column("owner_id", .integer).notNull()
                t.column("role", .text).notNull()
                // Where it sat, so a cached screen comes back in the order the server
                // sent rather than in whatever order the rows are read.
                t.column("position", .integer).notNull()
            }

            try db.create(table: "items") { t in
                t.column("id", .integer).primaryKey()
                t.column("uuid", .text).notNull()
                t.column("list_id", .integer).notNull().indexed()
                t.column("name", .text).notNull()
                t.column("amount", .double).notNull()
                t.column("unit_id", .integer)
                t.column("done_at", .datetime)
                // Comma-separated. A join table for a cache of one page would be three
                // queries to rebuild what the server sends as one array.
                t.column("tag_ids", .text).notNull()
                t.column("position", .integer).notNull()
            }

            // Units and tags, cached for the same reason: a list read with no signal
            // should still be measured and filed, not a column of bare names.
            // `list_id` is 0 for units, which are the same everywhere, and the list's
            // own id for tags, whose order is resolved per person and per list.
            try db.create(table: "reference") { t in
                t.column("kind", .text).notNull()
                t.column("list_id", .integer).notNull()
                t.column("id", .integer).notNull()
                t.column("name", .text).notNull()
                t.column("emoji", .text)
                t.column("position", .integer).notNull()
                t.primaryKey(["kind", "list_id", "id"])
            }
        }

        migrator.registerMigration("v2-outbox") { db in
            try db.create(table: "operations") { t in
                // Autoincrementing, so a sequence number is never reused. Reuse would
                // put a new operation in a gap left by an old one, which is the one
                // thing a device's own ordering may not do.
                t.autoIncrementedPrimaryKey("sequence")
                t.column("id", .text).notNull().unique()
                t.column("kind", .text).notNull()
                t.column("list_id", .integer).notNull().indexed()
                t.column("list_uuid", .text).notNull()
                t.column("item_id", .integer).notNull()
                t.column("item_uuid", .text).notNull()
                t.column("payload", .text).notNull()
                t.column("at", .datetime).notNull()
            }
        }

        // What this person buys, and what they file it under.
        //
        // The server keeps one of these per person per list and uses it for two
        // things: autocomplete, and filling in what a re-typed line does not say --
        // `milk` arrives in pints, under dairy, having typed four letters. On a device
        // with no server neither happened, because neither had anywhere to live.
        // Re-adding something you had just removed brought it back bare.
        //
        // **Not disposable, unlike the rest of the cache.** A server can hand back the
        // lists; nothing can hand back a history the device built on its own. It shares
        // the file with the outbox for that reason, and is migrated rather than dropped.
        migrator.registerMigration("v3-history") { db in
            try db.create(table: "history") { t in
                t.column("list_id", .integer).notNull()
                // Stored lowercased and matched that way: somebody typing `Milk` today
                // and `milk` tomorrow means the same habit.
                //
                // Keyed by the pair, because the same name on two lists is two habits:
                // milk in pints at home, milk in litres for the office.
                t.column("name", .text).notNull()
                t.column("unit_id", .integer)
                t.column("tag_ids", .text).notNull().defaults(to: "")
                t.column("uses", .integer).notNull().defaults(to: 0)
                // Unix seconds, which is what the shared ranking policy wants.
                t.column("last_used_at", .integer).notNull().defaults(to: 0)
                t.primaryKey(["list_id", "name"])
            }
        }

        // How much of it you usually buy. The memory already held the unit, so
        // `apples` came back in kilos and then asked how many -- every week, for
        // something bought two kilos at a time every week.
        //
        // Its own migration rather than a change to `v3-history`: a device that has
        // already run that one has a history worth keeping.
        migrator.registerMigration("v4-history-amount") { db in
            try db.alter(table: "history") { t in
                // Nullable: a name remembered before this knows its unit and not its
                // amount, and inventing one for it would be inventing a fact.
                t.add(column: "amount", .double)
            }
        }

        try? migrator.migrate(queue)
    }

    // MARK: - What this person buys

    /// One remembered line: what it was called, and what it turned out to be.
    struct Remembered: Equatable {
        var name: String
        var unitID: Int64?
        /// How much of it was last bought, if this name has been bought since the
        /// memory learned to hold a number.
        var amount: Double?
        var tagIDs: [Int64]
        var uses: Int64
        /// Unix seconds.
        var lastUsedAt: Int64
    }

    /// Records that this was bought, and what it was.
    ///
    /// Called after an add and after an edit, which is where the server records it too:
    /// what somebody corrected an item *to* is a better memory than what they first
    /// typed. The count only rises on an add -- editing one row twice is one intention,
    /// not two.
    func remember(_ item: Item, on list: List, isNew: Bool) {
        let tags = item.tagIDs.map(String.init).joined(separator: ",")
        let keepOldTags = tags.isEmpty && isNew
        write { db in
            try db.execute(
                sql: HistorySQL.remember,
                arguments: [
                    list.id,
                    item.name.lowercased(),
                    item.unitID,
                    item.amount,
                    tags,
                    isNew ? 1 : 0,
                    Int64(Date().timeIntervalSince1970),
                    keepOldTags,
                    isNew ? 1 : 0,
                ]
            )
        }
    }

    /// What this list's history knows about a name, if anything.
    func remembered(_ name: String, on list: List) -> Remembered? {
        read { db in
            try Row.fetchAll(
                db,
                sql: "SELECT * FROM history WHERE list_id = ? AND name = ?",
                arguments: [list.id, name.lowercased()]
            ).map(Self.remembered)
        }.first
    }

    /// Everything this list's history holds, for the ranker to sort.
    func history(on list: List) -> [Remembered] {
        read { db in
            try Row.fetchAll(
                db,
                sql: "SELECT * FROM history WHERE list_id = ?",
                arguments: [list.id]
            ).map(Self.remembered)
        }
    }


    /// Takes the server's memory as this device's own.
    ///
    /// A replace, like the item cache: what the server holds is the household's memory
    /// and this device's copy is a copy. Anything added here that has not drained yet
    /// is not lost by it -- the operation is still in the outbox, and the server will
    /// record it and send it back next time.
    ///
    /// Kept rather than merged for that reason. Merging would mean deciding whose
    /// count and whose last-used wins, which is a conflict rule for something that has
    /// an authority: the server has one memory per list and this is it.
    func adopt(history entries: [RememberedEntry], on list: List) {
        write { db in
            try db.execute(
                sql: "DELETE FROM history WHERE list_id = ?",
                arguments: [list.id]
            )
            for entry in entries {
                try db.execute(
                    sql: """
                    INSERT INTO history
                        (list_id, name, unit_id, amount, tag_ids, uses, last_used_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [
                        list.id,
                        entry.name.lowercased(),
                        entry.unitID,
                        entry.amount,
                        entry.tags.map(String.init).joined(separator: ","),
                        entry.uses,
                        entry.lastUsedAt,
                    ]
                )
            }
        }
    }

    /// Forgets one remembered line -- the way back from a typo, as the server has.
    func forget(_ name: String, on list: List) {
        write { db in
            try db.execute(
                sql: "DELETE FROM history WHERE list_id = ? AND name = ?",
                arguments: [list.id, name.lowercased()]
            )
        }
    }

    private static func remembered(_ row: Row) -> Remembered {
        let tags: String = row["tag_ids"]
        return Remembered(
            name: row["name"],
            unitID: row["unit_id"],
            amount: row["amount"],
            tagIDs: tags.split(separator: ",").compactMap { Int64($0) },
            uses: row["uses"],
            lastUsedAt: row["last_used_at"]
        )
    }


    // MARK: - Lists

    func lists() -> [List] {
        read { db in
            try Row.fetchAll(db, sql: "SELECT * FROM lists ORDER BY position").map { row in
                List(
                    id: row["id"],
                    uuid: row["uuid"],
                    name: row["name"],
                    ownerID: row["owner_id"],
                    role: Role(rawValue: row["role"]) ?? .viewer
                )
            }
        }
    }

    func remember(lists: [List]) {
        defer { announce() }

        // One transaction, because delete-then-insert has a moment where the cache
        // says there are no lists, and a read landing in that moment is the very bug
        // this table exists to prevent.
        write { db in
            // Lists this device made and has not managed to send keep their rows. The
            // server has never heard of them, so it cannot mention them, and deleting
            // everything it did not mention would take somebody's shopping away for
            // the crime of having been written down offline.
            try db.execute(sql: "DELETE FROM lists WHERE id >= 0")
            for (at, list) in lists.enumerated() {
                try db.execute(
                    sql: """
                    INSERT INTO lists (id, uuid, name, owner_id, role, position)
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [list.id, list.uuid, list.name, list.ownerID, list.role.rawValue, at]
                )
            }
        }
    }

    // MARK: - Items

    /// Makes a list here, with no server involved.
    ///
    /// The id is negative and minted locally, which is the same trick items already
    /// use for rows created offline: it is a key for this device's own tables and
    /// never goes on the wire, where the `uuid` is the only name. When the server
    /// finally hears about it, [`adopt`] swaps the one for the other.
    ///
    /// Counting down from the lowest already used, so two lists made in the same
    /// second cannot collide.
    func makeListHere(named name: String, ownedBy ownerID: Int64) -> List {
        let list = List(
            id: nextLocalListID(),
            uuid: UUID().uuidString.lowercased(),
            name: name,
            ownerID: ownerID,
            role: .owner
        )

        write { db in
            let position = try Int.fetchOne(db, sql: "SELECT count(*) FROM lists") ?? 0
            try db.execute(
                sql: """
                INSERT INTO lists (id, uuid, name, owner_id, role, position)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                arguments: [list.id, list.uuid, list.name, list.ownerID, list.role.rawValue, position]
            )
        }

        return list
    }

    private func nextLocalListID() -> Int64 {
        let lowest = readOne { db in
            try Int64.fetchOne(db, sql: "SELECT min(id) FROM lists")
        } ?? 0

        return min(lowest, 0) - 1
    }

    /// Whether this list exists only here.
    static func isLocal(_ list: List) -> Bool { list.id < 0 }

    /// Gives a locally-made list the id the server gave it.
    ///
    /// Everything keyed by the old id moves with it: the items on it, the tag order
    /// remembered for it, and anything still queued against it. Missing one of those
    /// would leave rows pointing at a list id that no longer exists, which reads on
    /// screen as a list that lost its items the moment it was first synced.
    ///
    /// The `uuid` does not change and never has — it is what the server was told, and
    /// what every queued operation names. Only this device's own numbering moves.
    func adopt(_ local: List, as real: List) {
        guard Self.isLocal(local), !Self.isLocal(real) else { return }

        write { db in
            for statement in [
                "UPDATE lists SET id = ?2, owner_id = ?3 WHERE id = ?1",
                "UPDATE items SET list_id = ?2 WHERE list_id = ?1",
                "UPDATE reference SET list_id = ?2 WHERE list_id = ?1",
                "UPDATE operations SET list_id = ?2 WHERE list_id = ?1",
            ] {
                try db.execute(sql: statement, arguments: [local.id, real.id, real.ownerID])
            }
        }
    }

    func items(on list: List) -> [Item] {
        read { db in
            try Row.fetchAll(
                db,
                sql: "SELECT * FROM items WHERE list_id = ? ORDER BY position",
                arguments: [list.id]
            ).map { row in
                let tags: String = row["tag_ids"]
                return Item(
                    id: row["id"],
                    uuid: row["uuid"],
                    name: row["name"],
                    amount: row["amount"],
                    unitID: row["unit_id"],
                    doneAt: row["done_at"],
                    tagIDs: tags.split(separator: ",").compactMap { Int64($0) }
                )
            }
        }
    }

    func remember(items: [Item], on list: List) {
        defer { announce() }

        write { db in
            try db.execute(sql: "DELETE FROM items WHERE list_id = ?", arguments: [list.id])
            for (at, item) in items.enumerated() {
                try db.execute(
                    sql: """
                    INSERT INTO items
                        (id, uuid, list_id, name, amount, unit_id, done_at, tag_ids, position)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [
                        item.id, item.uuid, list.id, item.name, item.amount, item.unitID,
                        item.doneAt, item.tagIDs.map(String.init).joined(separator: ","), at,
                    ]
                )
            }
        }
    }

    // MARK: - Units and tags

    func units() -> [Unit] {
        reference(kind: Kind.unit, listID: Kind.global).map { Unit(id: $0.id, name: $0.name) }
    }

    func remember(units: [Unit]) {
        replaceReference(
            kind: Kind.unit,
            listID: Kind.global,
            rows: units.map { (id: $0.id, name: $0.name, emoji: nil) }
        )
    }

    func tags(on list: List) -> [Tag] {
        // `sortOrder` is filled from the stored position rather than from the server's
        // own column: position already holds the order this person resolved, and a
        // stored zero would read as a tie everywhere downstream.
        reference(kind: Kind.tag, listID: list.id).enumerated().map { at, row in
            Tag(id: row.id, name: row.name, emoji: row.emoji, sortOrder: Int64(at))
        }
    }

    func remember(tags: [Tag], on list: List) {
        replaceReference(
            kind: Kind.tag,
            listID: list.id,
            rows: tags.map { (id: $0.id, name: $0.name, emoji: $0.emoji) }
        )
    }

    /// Everything this person had. Called on sign-out: the next person to sign in on
    /// this device is a different person, and must not be shown somebody else's
    /// shopping.
    func forgetEverything() {
        write { db in
            try db.execute(sql: "DELETE FROM lists")
            try db.execute(sql: "DELETE FROM items")
            try db.execute(sql: "DELETE FROM reference")
        }
        outbox.forgetEverything()
    }

    // MARK: - Plumbing

    private enum Kind {
        static let unit = "unit"
        static let tag = "tag"
        /// `list_id` for rows belonging to no list.
        static let global: Int64 = 0
    }

    private func reference(
        kind: String,
        listID: Int64
    ) -> [(id: Int64, name: String, emoji: String?)] {
        read { db in
            try Row.fetchAll(
                db,
                sql: "SELECT * FROM reference WHERE kind = ? AND list_id = ? ORDER BY position",
                arguments: [kind, listID]
            ).map { (id: $0["id"], name: $0["name"], emoji: $0["emoji"]) }
        }
    }

    private func replaceReference(
        kind: String,
        listID: Int64,
        rows: [(id: Int64, name: String, emoji: String?)]
    ) {
        write { db in
            try db.execute(
                sql: "DELETE FROM reference WHERE kind = ? AND list_id = ?",
                arguments: [kind, listID]
            )
            for (at, row) in rows.enumerated() {
                try db.execute(
                    sql: """
                    INSERT INTO reference (kind, list_id, id, name, emoji, position)
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [kind, listID, row.id, row.name, row.emoji, at]
                )
            }
        }
    }

    /// One value rather than rows. `read` answers with a collection because almost
    /// everything here does; this is for the handful of questions that have a single
    /// answer, and `nil` where the database could not be reached at all.
    private func readOne<T>(_ work: (Database) throws -> T?) -> T? {
        guard let queue else { return nil }
        return (try? queue.read(work)) ?? nil
    }

    private func read<T>(_ work: (Database) throws -> [T]) -> [T] {
        guard let queue else { return [] }
        return (try? queue.read(work)) ?? []
    }

    private func write(_ work: @escaping (Database) throws -> Void) {
        guard let queue else { return }
        try? queue.write(work)
    }

    /// Says the cache changed, because it is a database and nothing observes a
    /// database.
    ///
    /// One announcement rather than a call at each of the places that write, because
    /// the places that write are the places somebody adds a fourth of without knowing
    /// that anything downstream cared. What listens today is the watch link -- the
    /// phone is the watch's server, so a change here is news the wrist is waiting for.
    private func announce() {
        NotificationCenter.default.post(name: .cacheChanged, object: nil)
    }
}

extension Notification.Name {
    /// The lists or items this device holds have changed.
    static let cacheChanged = Notification.Name("shoppinglist.cacheChanged")
}

/// The one statement worth naming, because it carries a rule rather than a shape.
private enum HistorySQL {
    /// Upsert, with one conditional: an **add** that mentions no aisle must not erase
    /// what the last one learned, while an **edit** that clears them is somebody
    /// saying "not there" and is obeyed.
    static let remember = """
        INSERT INTO history (list_id, name, unit_id, amount, tag_ids, uses, last_used_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(list_id, name) DO UPDATE SET
            unit_id = COALESCE(excluded.unit_id, history.unit_id),
            amount = COALESCE(excluded.amount, history.amount),
            tag_ids = CASE WHEN ? THEN history.tag_ids ELSE excluded.tag_ids END,
            uses = history.uses + ?,
            last_used_at = excluded.last_used_at
        """
}
