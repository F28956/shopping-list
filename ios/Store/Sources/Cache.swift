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

    /// Opens the cache in Application Support, or gives up quietly.
    ///
    /// A `nil` queue is a working app with no memory rather than a crash on launch:
    /// the only thing this holds is a copy of what the server has, so a disk that
    /// will not cooperate costs a blank screen with no signal and nothing else.
    init(named name: String = "cache.sqlite") {
        queue = Self.open(named: name)
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
    /// `eraseDatabaseOnSchemaChange` is deliberate and is safe only while this holds
    /// nothing but a copy: throwing the cache away costs one load with no signal after
    /// an upgrade. The outbox, when it lands, is not disposable this way — it will hold
    /// work that exists nowhere else — and will need migrations written by hand.
    private func migrate() {
        guard let queue else { return }

        var migrator = DatabaseMigrator()
        #if DEBUG
        migrator.eraseDatabaseOnSchemaChange = true
        #endif

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

        try? migrator.migrate(queue)
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
        // One transaction, because delete-then-insert has a moment where the cache
        // says there are no lists, and a read landing in that moment is the very bug
        // this table exists to prevent.
        write { db in
            try db.execute(sql: "DELETE FROM lists")
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

    private func read<T>(_ work: (Database) throws -> [T]) -> [T] {
        guard let queue else { return [] }
        return (try? queue.read(work)) ?? []
    }

    private func write(_ work: @escaping (Database) throws -> Void) {
        guard let queue else { return }
        try? queue.write(work)
    }
}
