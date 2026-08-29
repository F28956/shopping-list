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
/// ## Why `@unchecked Sendable`
///
/// `unchecked` is a promise made to the compiler, so it is written down here rather
/// than left for somebody to re-derive. It rests on two things, and both are checkable
/// by reading this file:
///
/// 1. **There is no mutable state.** `queue` and `outbox` are both `let`, set once in
///    `init` and never reassigned. Nothing else is stored.
/// 2. **Every read and write goes through `DatabaseQueue`**, which serialises access
///    across threads -- that is what GRDB's queue is for. Two callers on two threads
///    cannot be inside the database at once.
///
/// It stops being true the moment a `var` is added here, or a caller is handed the
/// `DatabaseQueue` to use outside `read`/`write`. Either of those needs an actor
/// instead, not a repeat of this comment.
final class Cache: @unchecked Sendable {

    private let queue: DatabaseQueue?

    /// The queue of changes that have not been sent, in the same file — see ``Outbox``.
    let outbox: Outbox

    /// Whether there is a server to send to. See ``Outbox/sending``.
    private let sending: @Sendable () -> Bool

    /// Opens the cache in Application Support, or gives up quietly.
    ///
    /// A `nil` queue is a working app with no memory rather than a crash on launch:
    /// the only thing this holds is a copy of what the server has, so a disk that
    /// will not cooperate costs a blank screen with no signal and nothing else.
    init(
        named name: String = "cache.sqlite",
        sending: @escaping @Sendable () -> Bool = { !ServerDirectory.isOnDeviceOnly }
    ) {
        queue = Self.open(named: name)
        self.sending = sending
        outbox = Outbox(queue: queue, sending: sending)
        migrate()
        discardWhatCannotBeSent()
    }

    /// A cache at an exact path, for tests that need one to outlive the object — the
    /// queue surviving the app being killed is the whole point of it, and that is only
    /// checkable by opening the same file twice.
    init(
        path: String,
        sending: @escaping @Sendable () -> Bool = { !ServerDirectory.isOnDeviceOnly }
    ) {
        queue = try? DatabaseQueue(path: path)
        self.sending = sending
        outbox = Outbox(queue: queue, sending: sending)
        migrate()
        discardWhatCannotBeSent()
    }

    /// An in-memory cache, for tests and for `-uiTesting`, which must not read or
    /// write whatever the person running it happens to have on disk.
    /// Empties a queue that cannot ever be emptied any other way.
    ///
    /// On a device with no server the queue is always empty -- see the guard in
    /// `Outbox.queue`. It was not always so, and an app updated from a version that
    /// queued regardless is carrying operations addressed to nobody: unsendable,
    /// unprunable, and growing for as long as that version ran.
    ///
    /// Safe because `sending` is about whether a server has been *chosen*, not whether
    /// it can be reached. A device in server mode with no signal keeps its queue, which
    /// is the whole point of having one.
    ///
    /// Nothing is lost. What those operations describe is in the cache already, which is
    /// where this device reads from, and `handOverIfNeeded` rebuilds what a server needs
    /// from that -- from the current state rather than from the history of it.
    private func discardWhatCannotBeSent() {
        guard !sending(), outbox.waiting > 0 else { return }
        outbox.forgetEverything()
    }

    /// An in-memory cache, for tests and for `-uiTesting`.
    ///
    /// `sending` decides whether changes are queued, which is the difference between
    /// standalone and server mode -- see ``Outbox/sending``. Tests say which they mean
    /// rather than inheriting whatever the machine running them happens to be set to.
    static func inMemory(
        sending: @escaping @Sendable () -> Bool = { !ServerDirectory.isOnDeviceOnly }
    ) -> Cache {
        Cache(named: "", sending: sending)
    }

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
        #if os(watchOS)
            // A watch always has somewhere to send, and that is the whole of
            // `PhoneDestination`: with a server the queue goes to the server, and with
            // none it goes to the phone, which is this watch's server. The default here
            // is the *phone's* rule -- "nobody to tell, so queue nothing" -- and on the
            // wrist it is simply false. It meant every tick made on a watch paired to a
            // standalone phone was dropped on the floor: the row greyed out, the queue
            // stayed empty, nothing was ever sent, and the next snapshot from the phone
            // put the row back. Silently, because the dot and the unsent marks are both
            // hidden without a server.
            return Cache(sending: { true })
        #else
            return Cache()
        #endif
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
        // Whether a unit means something written with no number in front of it.
        //
        // The reference table held a name and an emoji, and `bare` was dropped on the
        // way in and read back as false for everything -- so `pint milk` was a unit of
        // "pint milk" on any device that had cached its units, which is every device
        // after its first run. The rule was right and the storage forgot its input.
        migrator.registerMigration("v5-units-that-stand-alone") { db in
            try db.alter(table: "reference") { t in
                t.add(column: "bare", .boolean).notNull().defaults(to: false)
            }
        }

        // Throws the cached units away, so they are read again with the flag set.
        //
        // A new column defaults every existing row to `false`, which is the same wrong
        // answer the missing column gave -- and units are only re-read when the cache
        // is empty, so without this they would stay wrong for ever on any device that
        // had opened a list once.
        //
        // **Its own migration, and that is the point.** This began as two more lines
        // inside the one above, which had already run: a migrator records what it has
        // applied and never runs it again, so the delete simply did not happen and the
        // units stayed as they were. A fix to an applied migration is a new migration.
        //
        // Safe because units are the server's to give, or the bundle's: unlike the
        // aisles beside them in this table, nothing about them is somebody's own
        // choice. The walking order is, which is why only units go.
        // The spelling last used, beside the key it is stored under.
        //
        // The key is folded so `Milk` and `milk ` are one memory, and offering that key
        // back meant every suggestion arrived lowercase where the server's arrive as
        // somebody wrote them. Two columns for the same reason the server has two.
        migrator.registerMigration("v7-history-display") { db in
            try db.alter(table: "history") { t in
                t.add(column: "display", .text).notNull().defaults(to: "")
            }
            // Backfilled from the key, which is the best that is known: it is what has
            // been shown all along, and the next use writes the real spelling.
            try db.execute(sql: "UPDATE history SET display = name WHERE display = ''")
        }

        migrator.registerMigration("v6-reread-units") { db in
            try db.execute(sql: "DELETE FROM reference WHERE kind = 'unit'")
        }

        migrator.registerMigration("v4-history-amount") { db in
            try db.alter(table: "history") { t in
                // Nullable: a name remembered before this knows its unit and not its
                // amount, and inventing one for it would be inventing a fact.
                t.add(column: "amount", .double)
            }
        }

        // The categories, stored the way the server stores them.
        //
        // They were in `reference` keyed on `(kind, list_id, id)`, which is one copy of
        // the whole vocabulary per list. That is not what a category is: there is one
        // set of them, and what a list has is an *order* over it. The server has said so
        // since the beginning -- `tags` is a single table there and `list_tag_order`
        // holds nothing but positions -- and only this cache flattened the two together.
        //
        // What the flattening cost: `allTags` had to pick a list to read the vocabulary
        // off and answered nothing when there were none; renaming, adding and removing
        // each had to loop every list and rewrite its rows; two lists could disagree
        // about a name with nothing to stop them; and the storage was O(lists x tags)
        // for something that is O(tags).
        migrator.registerMigration("v8-one-vocabulary") { db in
            try db.execute(
                sql: """
                    CREATE TABLE tags (
                        id       INTEGER PRIMARY KEY,
                        name     TEXT NOT NULL,
                        emoji    TEXT,
                        -- Where this category falls when no list has said otherwise.
                        -- The screen that manages them belongs to no list, so it needs
                        -- an order of its own rather than borrowing one.
                        position INTEGER NOT NULL
                    )
                    """
            )
            try db.execute(
                sql: """
                    CREATE TABLE list_tag_order (
                        list_id  INTEGER NOT NULL,
                        tag_id   INTEGER NOT NULL,
                        position INTEGER NOT NULL,
                        PRIMARY KEY (list_id, tag_id)
                    ) WITHOUT ROWID
                    """
            )

            // Carried over rather than re-fetched: a device with no server has nowhere
            // to re-fetch from, and the rows it holds are the only copy in the world.
            //
            // The lowest list_id wins where the copies disagree, which they should not.
            // `MIN` rather than an arbitrary pick so the result does not depend on the
            // order rows come back in.
            try db.execute(
                sql: """
                    INSERT INTO tags (id, name, emoji, position)
                    SELECT id, name, emoji, MIN(position)
                      FROM reference WHERE kind = 'tag'
                     GROUP BY id
                    """
            )
            try db.execute(
                sql: """
                    INSERT INTO list_tag_order (list_id, tag_id, position)
                    SELECT list_id, id, position FROM reference WHERE kind = 'tag'
                    """
            )
            try db.execute(sql: "DELETE FROM reference WHERE kind = 'tag'")
        }

        // A migration that fails leaves the app running on whatever schema it had:
        // reads answer `[]`, writes roll back, and the queue in this same file exists
        // nowhere else -- a device that acknowledges every change on screen and stores
        // none of them. Loud here for that reason.
        do {
            try migrator.migrate(queue)
        } catch {
            noted(error, "migrate")
        }
    }

    // MARK: - What this person buys

    /// One remembered line: what it was called, and what it turned out to be.
    struct Remembered: Equatable {
        /// The key: trimmed and lowercased, so one habit has one entry.
        var name: String
        /// The spelling last used, which is what to show back.
        var display: String
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
                    foldedName(item.name),
                    item.name.trimmingCharacters(in: .whitespacesAndNewlines),
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
                // Trimmed as well as folded, matching `parsing::add::fold` — the one
                // key a name is remembered under. Lowercasing alone is the same answer
                // for every name anybody actually types and a different one the moment
                // somebody types a trailing space.
                arguments: [list.id, foldedName(name)]
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
                        (list_id, name, display, unit_id, amount, tag_ids, uses,
                         last_used_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [
                        list.id,
                        foldedName(entry.name),
                        entry.display,
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
                arguments: [list.id, foldedName(name)]
            )
        }
    }

    private static func remembered(_ row: Row) -> Remembered {
        let tags: String = row["tag_ids"]
        return Remembered(
            name: row["name"],
            display: row["display"],
            unitID: row["unit_id"],
            amount: row["amount"],
            tagIDs: tags.split(separator: ",").compactMap { Int64($0) },
            uses: row["uses"],
            lastUsedAt: row["last_used_at"]
        )
    }


    // MARK: - Lists

    func lists() -> [List] {
        read { db in try Self.fetchLists(db) }
    }

    fileprivate static func fetchLists(_ db: Database) throws -> [List] {
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

    func remember(lists: [List]) {
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
                // `OR REPLACE`, because a row being written again is the ordinary case
                // rather than a mistake. The delete above spares negative ids -- lists
                // made here and not yet sent -- and on the watch *every* list has one,
                // minted locally because there is no server to have minted a real one.
                // A plain insert therefore hit the primary key on the second snapshot
                // and threw, which took the whole transaction with it: the watch's
                // picture froze on the first thing it was ever told and no later
                // snapshot could correct it, with nothing on screen to say so.
                try db.execute(
                    sql: """
                    INSERT OR REPLACE INTO lists (id, uuid, name, owner_id, role, position)
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

    /// Drops the lists this device kept from before its own server took over.
    ///
    /// `LocalBackend.readyForUse` copies the cache into `device.sqlite` and leaves the
    /// cache exactly as it was, deliberately, so the move stays reversible. Once that
    /// has happened those rows are a photograph of a moment: every edit since went to
    /// `device.sqlite`, and the two copies do not even share uuids, because the
    /// migration mints new ones for the lists.
    ///
    /// So handing the device to a server without dropping them first would queue both
    /// copies, and the server would be told about the same shopping twice under two
    /// different names -- which is what a real Mac was one relaunch away from doing.
    ///
    /// Only the local ones. A list with a server's id is that server's, and this is
    /// called at the moment one is adopted.
    func forgetLocalLists() {
        write { db in
            for table in ["items", "history", "list_tag_order"] {
                try db.execute(
                    sql: "DELETE FROM \(table) WHERE list_id IN (SELECT id FROM lists WHERE id < 0)"
                )
            }
            try db.execute(sql: "DELETE FROM lists WHERE id < 0")
        }
    }

    /// Takes lists and their rows in as this device's own, keeping the names a server
    /// will be told.
    ///
    /// The other end of `handOverIfNeeded`. That walks the cache for lists no server has
    /// heard of and queues them; this is how the lists on a device that has been
    /// answering for *itself* get into the cache to be walked. Without it, adopting a
    /// server showed an empty account with a year of shopping still on disk: everything
    /// lives in `device.sqlite`, `handOverIfNeeded` reads `cache.sqlite`, and nothing
    /// joined the two.
    ///
    /// The uuids come in rather than being minted, unlike `makeListHere`. They are what
    /// every queued operation names and what the server records, and a new one here
    /// would make the same list twice the first time these two ever met. The ids are
    /// local because that is precisely what "no server has heard of this" is written as.
    ///
    /// One transaction: a half-written handover is a device that would queue some of
    /// somebody's shopping and quietly drop the rest.
    func takeIn(_ incoming: [(list: List, items: [Item])]) {
        guard !incoming.isEmpty else { return }

        var nextList = nextLocalListID()
        var nextItem = readOne { db in
            try Int64.fetchOne(db, sql: "SELECT min(id) FROM items")
        } ?? 0
        nextItem = min(nextItem, 0)

        write { db in
            var position = try Int.fetchOne(db, sql: "SELECT count(*) FROM lists") ?? 0

            for (list, items) in incoming {
                // Already here, so this has run before or the two stores overlap. Left
                // alone rather than written twice.
                let known = try Int64.fetchOne(
                    db,
                    sql: "SELECT count(*) FROM lists WHERE uuid = ?",
                    arguments: [list.uuid]
                ) ?? 0
                guard known == 0 else { continue }

                try db.execute(
                    sql: """
                    INSERT INTO lists (id, uuid, name, owner_id, role, position)
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [
                        nextList, list.uuid, list.name, list.ownerID,
                        Role.owner.rawValue, position,
                    ]
                )

                for (at, item) in items.enumerated() {
                    nextItem -= 1
                    try db.execute(
                        sql: """
                        INSERT INTO items
                            (id, uuid, list_id, name, amount, unit_id, done_at, tag_ids,
                             position)
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                        """,
                        arguments: [
                            nextItem, item.uuid, nextList, item.name, item.amount,
                            item.unitID, item.doneAt,
                            item.tagIDs.map(String.init).joined(separator: ","), at,
                        ]
                    )
                }

                nextList -= 1
                position += 1
            }
        }
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
    /// remembered for it, what this list has taught the box, and anything still queued
    /// against it. Missing one of those would leave rows pointing at a list id that no
    /// longer exists, which reads on screen as a list that lost its items the moment it
    /// was first synced.
    ///
    /// Two of them were missing. `reference` held the tag order until `v8` split it
    /// into `tags` and `list_tag_order`, and this was not told; `history` was never
    /// here at all. So a list made offline, walked in somebody's own aisle order and
    /// added to for a fortnight, reached a server and arrived with its walking order
    /// and its entire memory orphaned under a number nothing points at any more --
    /// unreachable, undeletable, and gone from the suggestions.
    ///
    /// The `uuid` does not change and never has — it is what the server was told, and
    /// what every queued operation names. Only this device's own numbering moves.
    func adopt(_ local: List, as real: List) {
        guard Self.isLocal(local), !Self.isLocal(real) else { return }

        write { db in
            // The list first, then everything that points at it.
            //
            // Handed its own arguments, because the others take two and this takes
            // three -- and every statement was previously given all three, which GRDB
            // refuses as the wrong number for a statement that names two. The refusal
            // took the whole transaction with it, so this moved *nothing*: not the
            // history, not the walking order, and not the list either. It has never
            // worked, and nothing called it in a test.
            try db.execute(
                sql: "UPDATE lists SET id = ?2, owner_id = ?3 WHERE id = ?1",
                arguments: [local.id, real.id, real.ownerID]
            )
            for table in ["items", "reference", "list_tag_order", "history", "operations"] {
                try db.execute(
                    sql: "UPDATE \(table) SET list_id = ?2 WHERE list_id = ?1",
                    arguments: [local.id, real.id]
                )
            }
        }
    }

    func items(on list: List) -> [Item] {
        read { db in try Self.fetchItems(db, on: list) }
    }

    /// The fetch itself, so that a one-shot read and an observation of the same rows
    /// cannot come out different. Everything below `observe(list:)` reuses these.
    fileprivate static func fetchItems(_ db: Database, on list: List) throws -> [Item] {
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

    /// Drops lists that are not in the picture just received, and their rows.
    ///
    /// For a caller holding a *complete* picture, which the watch's snapshot is and the
    /// server's answer is not: `remember(lists:)` deliberately spares negative ids
    /// because on a phone they are lists made here and not yet sent, and deleting them
    /// would take somebody's shopping away for the crime of having been written down
    /// offline. On the watch every list has a negative id, so nothing was ever removed
    /// -- a list deleted on the phone stayed on the wrist for good, and a list whose
    /// uuid changed appeared twice under the same name.
    func forgetLists(outside uuids: Set<String>) {
        write { db in
            let holes = uuids.isEmpty ? "NULL" : uuids.map { _ in "?" }.joined(separator: ",")
            let arguments = StatementArguments(Array(uuids))
            try db.execute(
                sql: """
                DELETE FROM items WHERE list_id IN
                    (SELECT id FROM lists WHERE uuid NOT IN (\(holes)))
                """,
                arguments: arguments
            )
            try db.execute(
                sql: "DELETE FROM lists WHERE uuid NOT IN (\(holes))",
                arguments: arguments
            )
        }
    }

    /// Drops rows belonging to lists this device no longer has.
    ///
    /// `remember(items:on:)` clears a list by its id, so rows whose list has gone --
    /// or whose id has changed, which is what happened when a watch that had been on
    /// a server was handed to one that answers for itself -- are left behind with no
    /// list to reach them from. Invisible, permanent, and in the way.
    func forgetItems(outside lists: Set<Int64>) {
        write { db in
            let placeholders = lists.isEmpty
                ? "NULL"
                : lists.map { String($0) }.joined(separator: ",")
            try db.execute(sql: "DELETE FROM items WHERE list_id NOT IN (\(placeholders))")
        }
    }

    // MARK: - Units and tags

    func units() -> [Unit] {
        read { db in try Self.fetchUnits(db) }
    }

    fileprivate static func fetchUnits(_ db: Database) throws -> [Unit] {
        try Row.fetchAll(
            db,
            sql: "SELECT * FROM reference WHERE kind = ? AND list_id = ? ORDER BY position",
            arguments: [Kind.unit, Kind.global]
        ).map { Unit(id: $0["id"], name: $0["name"], bare: $0["bare"]) }
    }

    func remember(units: [Unit]) {
        replaceReference(
            kind: Kind.unit,
            listID: Kind.global,
            rows: units.map { (id: $0.id, name: $0.name, emoji: nil, bare: $0.bare) }
        )
    }

    /// The vocabulary in this list's order.
    ///
    /// A join, because those are two different things: `tags` is what the categories
    /// *are* and `list_tag_order` is where this list puts them. A category this list has
    /// never ordered still appears -- at its own position, which is what a list that has
    /// never been reordered gets.
    ///
    /// `sortOrder` is filled from the row's rank rather than from the stored position:
    /// position already holds the order this person resolved, and a stored zero would
    /// read as a tie everywhere downstream.
    func tags(on list: List) -> [Tag] {
        read { db in try Self.fetchTags(db, on: list) }
    }

    fileprivate static func fetchTags(_ db: Database, on list: List) throws -> [Tag] {
        try Row.fetchAll(
            db,
            sql: """
                SELECT t.id, t.name, t.emoji
                  FROM tags t
                  LEFT JOIN list_tag_order o ON o.tag_id = t.id AND o.list_id = ?
                 ORDER BY COALESCE(o.position, t.position), t.id
                """,
            arguments: [list.id]
        )
        .enumerated()
        .map { at, row in
            Tag(id: row["id"], name: row["name"], emoji: row["emoji"], sortOrder: Int64(at))
        }
    }

    /// What the server said this list's categories are, and in what order.
    ///
    /// Both halves, because the server sends both in one answer -- but into their own
    /// tables. The vocabulary is upserted rather than replaced: this list's answer is
    /// evidence about the categories, not the whole truth about them, and a list that
    /// happens to carry a subset must not delete the rest.
    func remember(tags: [Tag], on list: List) {
        write { db in
            for (at, tag) in tags.enumerated() {
                try db.execute(
                    sql: """
                        INSERT INTO tags (id, name, emoji, position) VALUES (?, ?, ?, ?)
                        ON CONFLICT(id) DO UPDATE SET name = excluded.name, emoji = excluded.emoji
                        """,
                    arguments: [tag.id, tag.name, tag.emoji, at]
                )
            }

            try db.execute(sql: "DELETE FROM list_tag_order WHERE list_id = ?", arguments: [list.id])
            for (at, tag) in tags.enumerated() {
                try db.execute(
                    sql: "INSERT INTO list_tag_order (list_id, tag_id, position) VALUES (?, ?, ?)",
                    arguments: [list.id, tag.id, at]
                )
            }
        }
    }


    // MARK: - The aisles, which belong to no one list

    /// Every category this device knows.
    ///
    /// One statement, and no list involved, which is the point of the shape: categories
    /// are global and lists carry an order over them. This used to walk `lists()`
    /// looking for one whose rows it could read the vocabulary off, and answered nothing
    /// when there were none -- so the screen for managing them opened empty on a device
    /// that had not made a list yet.
    ///
    /// The bundled set stands in while the table is empty. The categories are not a
    /// property of having a list or of having a server; they are what the app ships
    /// with, and `seedReference` writes them down the first time a list is opened.
    func allTags() -> [Tag] {
        let stored = read { db in
            try Row.fetchAll(db, sql: "SELECT id, name, emoji FROM tags ORDER BY position, id")
        }
        .enumerated()
        .map { at, row in
            Tag(id: row["id"], name: row["name"], emoji: row["emoji"], sortOrder: Int64(at))
        }
        return stored.isEmpty ? Reference.tags : stored
    }

    /// Renames an aisle, or changes its glyph, everywhere.
    ///
    /// Every list, because a tag is one thing that happens to be written down once per
    /// list. Renaming it on the list somebody happens to be looking at would leave the
    /// same id under two names, and the next screen would disagree with this one.
    ///
    /// Each list keeps its own order: this replaces rows in place rather than
    /// rewriting the sequence.
    func rename(tag id: Int64, to name: String, emoji: String?) {
        seedTagsIfEmpty()
        write { db in
            try db.execute(
                sql: "UPDATE tags SET name = ?, emoji = ? WHERE id = ?",
                arguments: [name, emoji, id]
            )
        }
    }

    /// Writes the shipped categories down, if nothing has yet.
    ///
    /// `allTags` answers with them either way, so a screen looks right before this runs
    /// -- but an UPDATE against an empty table changes nothing, and the rename would
    /// have looked like it worked and been gone on the next read. Anything that edits
    /// the vocabulary makes sure there is one first.
    private func seedTagsIfEmpty() {
        let stored = readOne { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM tags")
        } ?? 0
        guard stored == 0 else { return }

        write { db in
            for (at, tag) in Reference.tags.enumerated() {
                try db.execute(
                    sql: "INSERT OR IGNORE INTO tags (id, name, emoji, position) VALUES (?, ?, ?, ?)",
                    arguments: [tag.id, tag.name, tag.emoji, at]
                )
            }
        }
    }

    /// Adds an aisle to every list, at the end of each one's order.
    ///
    /// The id is this device's to mint when there is no server, and negative for the
    /// same reason a locally-made row's is: it is a placeholder, and a server that
    /// arrives later brings its own vocabulary with its own numbering.
    func addTag(named name: String, emoji: String?) -> Tag {
        seedTagsIfEmpty()
        let id = -Int64(Date().timeIntervalSince1970 * 1000)

        // At the end of the vocabulary, and of nothing else. A list that has never been
        // reordered picks this up from `tags.position`; one that has gets it at the end
        // too, because `tags(on:)` falls back to that position for a category the list
        // has no row for. Neither needs writing to.
        write { db in
            let last = try Int.fetchOne(db, sql: "SELECT COALESCE(MAX(position), -1) FROM tags") ?? -1
            try db.execute(
                sql: "INSERT INTO tags (id, name, emoji, position) VALUES (?, ?, ?, ?)",
                arguments: [id, name, emoji, last + 1]
            )
        }

        return Tag(id: id, name: name, emoji: emoji, sortOrder: 0)
    }

    /// Removes an aisle from every list, and unfiles whatever was in it.
    ///
    /// The unfiling is not a nicety: an item carrying the id of an aisle that no longer
    /// exists is filed nowhere the screen can show, and would sort as though it were
    /// still first. The server cascades exactly this on its side.
    func removeTag(_ id: Int64) {
        seedTagsIfEmpty()
        write { db in
            try db.execute(sql: "DELETE FROM tags WHERE id = ?", arguments: [id])
            // The order rows go with it. Nothing enforces that for us: the cache has no
            // foreign keys, because it is rebuilt from the server rather than trusted.
            try db.execute(sql: "DELETE FROM list_tag_order WHERE tag_id = ?", arguments: [id])
        }

        // The unfiling still walks the lists, and has to: it is the items that carry
        // the id, and they are per list by their nature rather than by an accident of
        // storage.
        for list in lists() {
            let items = items(on: list)
            let touched = items.filter { $0.tagIDs.contains(id) }
            guard !touched.isEmpty else { continue }

            remember(
                items: items.map { item in
                    guard item.tagIDs.contains(id) else { return item }
                    return Item(
                        id: item.id,
                        uuid: item.uuid,
                        name: item.name,
                        amount: item.amount,
                        unitID: item.unitID,
                        doneAt: item.doneAt,
                        tagIDs: item.tagIDs.filter { $0 != id }
                    )
                },
                on: list
            )
        }
    }

    /// Everything this person had. Called on sign-out: the next person to sign in on
    /// this device is a different person, and must not be shown somebody else's
    /// shopping.
    func forgetEverything() {
        write { db in
            try db.execute(sql: "DELETE FROM lists")
            try db.execute(sql: "DELETE FROM items")
            try db.execute(sql: "DELETE FROM reference")
            // The categories go too. They are one server's vocabulary, and this is the
            // moment somebody stops being on that server -- keeping them would show a
            // stranger's aisles under nobody's name. The bundled set answers until the
            // next server does.
            try db.execute(sql: "DELETE FROM tags")
            try db.execute(sql: "DELETE FROM list_tag_order")
            // And what the lists taught the box.
            //
            // This was kept, on the reasoning that a server can hand back the lists
            // but nothing can hand back a memory the device built on its own. That was
            // true when the memory belonged to the person and lived only here. It
            // stopped being true when it moved to the *list* -- `20260825160000`, "so
            // a household shares one" -- and the server began handing it back per list
            // like everything else.
            //
            // What was left instead was one person's shopping seeded into the next
            // person's suggestions on a shared device: names they never typed, arriving
            // measured and filed. Keyed by `list_id`, and every list has just been
            // deleted, so keeping it also kept rows belonging to nothing.
            try db.execute(sql: "DELETE FROM history")
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
    ) -> [(id: Int64, name: String, emoji: String?, bare: Bool)] {
        read { db in
            try Row.fetchAll(
                db,
                sql: "SELECT * FROM reference WHERE kind = ? AND list_id = ? ORDER BY position",
                arguments: [kind, listID]
            ).map {
                (id: $0["id"], name: $0["name"], emoji: $0["emoji"], bare: $0["bare"])
            }
        }
    }

    private func replaceReference(
        kind: String,
        listID: Int64,
        rows: [(id: Int64, name: String, emoji: String?, bare: Bool)]
    ) {
        write { db in
            try db.execute(
                sql: "DELETE FROM reference WHERE kind = ? AND list_id = ?",
                arguments: [kind, listID]
            )
            for (at, row) in rows.enumerated() {
                try db.execute(
                    sql: """
                    INSERT INTO reference (kind, list_id, id, name, emoji, bare, position)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    """,
                    arguments: [kind, listID, row.id, row.name, row.emoji, row.bare, at]
                )
            }
        }
    }

    /// One value rather than rows. `read` answers with a collection because almost
    /// everything here does; this is for the handful of questions that have a single
    /// answer, and `nil` where the database could not be reached at all.
    private func readOne<T>(_ work: (Database) throws -> T?) -> T? {
        guard let queue else { return nil }
        do {
            return try queue.read(work)
        } catch {
            noted(error, "read")
            return nil
        }
    }

    private func read<T>(_ work: (Database) throws -> [T]) -> [T] {
        guard let queue else { return [] }
        do {
            return try queue.read(work)
        } catch {
            // The empty answer this returns is the one the whole cache exists to avoid
            // -- "you have no lists" where the truth is "I could not find out". Worth
            // hearing about rather than rendering.
            noted(error, "read")
            return []
        }
    }

    /// Every write, and every write says so.
    ///
    /// The announcement used to be a `defer` at the top of the two functions somebody
    /// remembered to put it in. Four others had none -- renaming, adding and removing a
    /// category, and taking a list's categories from the server -- so removing `dairy`
    /// in Settings left it on screen in every list that was already open. Nothing was
    /// stale in the database; the screens were simply never told.
    ///
    /// That is not a bug to fix four times. A cache whose readers are told about some
    /// writes is worse than one that is never told, because it looks like it works. So
    /// the announcement lives at the single point every write goes through, and a
    /// seventh writer added tomorrow inherits it instead of having to know.
    private func write(_ work: @escaping (Database) throws -> Void) {
        guard let queue else { return }
        do {
            try queue.write(work)
        } catch {
            // A constraint failure rolls back the whole transaction, and the result
            // then looks exactly like a write that had nothing to do. The watch spent
            // days showing an empty list for that reason, and `adopt(_: as:)` shipped
            // never having worked at all.
            noted(error, "write")
        }
        announce()
    }

    // MARK: - Handing a standalone device over to a server

    /// Puts everything this device holds into the queue, once, because it has just been
    /// pointed at a server.
    ///
    /// This is what the outbox used to do continuously and should not have. On a device
    /// with no server there is nobody to tell, so nothing is queued -- see the guard in
    /// `Outbox.queue`. But a device that *adopts* a server has a real handover to do:
    /// lists and items that exist nowhere else, which the server has never heard of and
    /// so can never mention.
    ///
    /// One pass over what is actually here, rather than a replay of how it got here.
    /// The two arrive at the same place, and this one is bounded by the size of the
    /// list instead of by how long the app has been used -- and it cannot replay an
    /// item that was added and later deleted, because such an item is simply not here.
    ///
    /// Only locally-made lists. Anything with a server's own id came from a server and
    /// is not this device's to hand over.
    ///
    /// Called before every drain rather than at the moment a server is chosen, and that
    /// is deliberate. A server can be adopted from four screens -- the phone's settings
    /// and its sign-in, the Mac's settings, the watch's identity -- and a handover that
    /// each of them has to remember to ask for is a handover three of them will
    /// eventually forget. This asks the only question that matters, at the only moment
    /// it matters: is there a server, and is there anything here it has never heard of?
    ///
    /// Nothing to do in the ordinary case, which is why it is cheap enough to sit in
    /// front of every drain: a device that never had local lists has none, and one that
    /// has already handed over finds its lists already queued.
    func handOverIfNeeded() {
        guard sending() else { return }

        // Already spoken for. A list stops being local the moment the server answers
        // with an id for it -- see `adopt` -- so this only skips a handover that is
        // still in flight, never one that is needed.
        let alreadyQueued = Set(outbox.all().map(\.listUUID))

        for list in lists() where Self.isLocal(list) && !alreadyQueued.contains(list.uuid) {
            outbox.makeList(list)

            for item in items(on: list) {
                outbox.add(
                    uuid: item.uuid,
                    localID: item.id,
                    name: item.name,
                    amount: item.amount,
                    unitID: item.unitID,
                    on: list
                )
                for tagID in item.tagIDs {
                    outbox.tag(item, on: list, tagID: tagID, attached: true)
                }
                // After the add, and only when it is true: the wire has no field for it
                // on an add, and the queue is ordered so this lands behind.
                if item.isDone {
                    outbox.setDone(item, on: list, done: true, at: item.doneAt ?? Date())
                }
            }
        }
    }

    // MARK: - Watching

    /// Everything one list's screen reads, in one answer.
    ///
    /// One struct rather than three observations, and that is the point: a screen given
    /// new items and old categories has a moment where a row is filed under an aisle
    /// that no longer exists. Fetched in a single read, so there is no such moment.
    struct ListContents: Equatable, Sendable {
        var items: [Item]
        var units: [Unit]
        var tags: [Tag]
    }

    /// Everything the lists screen reads.
    ///
    /// The queue count belongs here rather than beside it: it lives in the same database
    /// and it is part of the same answer. Keeping it out is what left both list screens
    /// re-reading `outbox.waiting` on a two-second timer -- a poll, to notice something
    /// the database could have told them.
    struct Overview: Equatable, Sendable {
        var lists: [List]
        var waiting: Int
    }

    func overview() -> Overview? {
        readOne { db in
            Overview(
                lists: try Self.fetchLists(db),
                waiting: try Self.fetchWaiting(db)
            )
        }
    }

    /// The lists, and how much is queued, for as long as somebody is looking.
    ///
    /// See `observe(list:)` for why this shape, and for where it stops seeing.
    func observeLists() -> AsyncValueObservation<Overview>? {
        guard let queue else { return nil }

        return ValueObservation
            .tracking { db in
                Overview(
                    lists: try Self.fetchLists(db),
                    waiting: try Self.fetchWaiting(db)
                )
            }
            .removeDuplicates()
            .values(in: queue)
    }

    fileprivate static func fetchWaiting(_ db: Database) throws -> Int {
        try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM operations") ?? 0
    }

    /// The same answer, once, for a caller that wants it in this turn rather than on
    /// the observation's next one -- a view appearing, or a test.
    func contents(of list: List) -> ListContents? {
        readOne { db in
            ListContents(
                items: try Self.fetchItems(db, on: list),
                units: try Self.fetchUnits(db),
                tags: try Self.fetchTags(db, on: list)
            )
        }
    }

    /// One list's screen, for as long as somebody is looking at it.
    ///
    /// This is what the app should have been doing all along. A screen used to hold a
    /// copy taken when it loaded, and staying level with the database was a protocol
    /// somebody had to follow by hand: write, remember to announce, remember to listen,
    /// remember to re-read the right things. Four steps, each skippable in silence, and
    /// removing a category skipped three of them at once.
    ///
    /// `ValueObservation` removes the protocol rather than documenting it. It knows
    /// which tables the fetch below touched, and re-runs it when any of them changes --
    /// whoever wrote, without being told. There is nothing for a new writer to remember
    /// and nothing for a new screen to subscribe to.
    ///
    /// **The boundary is this connection.** A `DatabaseQueue` observes writes made
    /// through itself, not every write to the file, so a second `Cache` over the same
    /// path is invisible to this one. That is sound here only because there is exactly
    /// one -- see `shared`, and the reason given there -- and every screen, the watch
    /// link and the outbox all go through it. It stops being sound the day something
    /// outside this process writes: a share extension, a widget, a background refresh.
    /// The fix then is a `DatabasePool` and cross-process notification, not another
    /// hand-posted `cacheChanged`. `aSecondConnectionIsNotObserved` pins this.
    ///
    /// The first value arrives immediately, so a caller does not need a separate read to
    /// put something on screen.
    func observe(list: List) -> AsyncValueObservation<ListContents>? {
        guard let queue else { return nil }

        return ValueObservation
            .tracking { db in
                ListContents(
                    items: try Self.fetchItems(db, on: list),
                    units: try Self.fetchUnits(db),
                    tags: try Self.fetchTags(db, on: list)
                )
            }
            // Identical values are not news. Ticking one row off rewrites that list's
            // rows, and without this every such write would push an unchanged set of
            // categories at the screen as well.
            .removeDuplicates()
            .values(in: queue)
    }

    /// Says the cache changed, because it is a database and nothing observes a
    /// database.
    ///
    /// One announcement rather than a call at each of the places that write, because
    /// the places that write are the places somebody adds a fourth of without knowing
    /// that anything downstream cared. What listens today is the watch link -- the
    /// phone is the watch's server, so a change here is news the wrist is waiting for.
    ///
    /// **On the main queue, always.** A notification is delivered synchronously on
    /// whichever thread posted it, and this is posted from whichever thread happened to
    /// be writing. Two of the three listeners are SwiftUI `.onReceive` closures that
    /// assign straight into `@State`, so posting from a database thread was a
    /// background write to view state -- the kind that works until the day it does not.
    /// Hopping here rather than at each listener means a fourth listener added later
    /// inherits the guarantee instead of having to know about it.
    private func announce() {
        if Thread.isMainThread {
            NotificationCenter.default.post(name: .cacheChanged, object: nil)
        } else {
            DispatchQueue.main.async {
                NotificationCenter.default.post(name: .cacheChanged, object: nil)
            }
        }
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
        INSERT INTO history
            (list_id, name, display, unit_id, amount, tag_ids, uses, last_used_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(list_id, name) DO UPDATE SET
            display = excluded.display,
            unit_id = COALESCE(excluded.unit_id, history.unit_id),
            amount = COALESCE(excluded.amount, history.amount),
            tag_ids = CASE WHEN ? THEN history.tag_ids ELSE excluded.tag_ids END,
            uses = history.uses + ?,
            last_used_at = excluded.last_used_at
        """
}
