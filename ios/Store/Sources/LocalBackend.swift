// See `LocalServer` for why this is absent on the watch.
#if !os(watchOS)

import EmbeddedC
import Foundation

/// The ``Backend`` a device answers for itself.
///
/// The other conformer is `API`, over HTTP. This one is over a database in this
/// device's own Application Support folder, run by `domain` -- the server's crate,
/// compiled for the phone. See `web/embedded`.
///
/// **The screens above it cannot tell which they have**, and that is the whole point.
/// Standalone is currently a server that fails every request, so every screen learned
/// to recognise a failure that was not real: `onDeviceOnly` reaches fifty-seven places
/// across eighteen files, the empty state has a special case, the status dot has one,
/// and the queue filled for a reader that did not exist. None of that is needed against
/// a backend that answers.
///
/// ## Two decisions taken deliberately
///
/// **Category names are shown as they are stored, which is lowercase.** `tag::Name`
/// normalises -- trimmed and folded -- so that `Dairy`, `dairy ` and `DAIRY` cannot
/// become three categories. The twenty-one shipped ones are stored that way already and
/// no screen capitalises them, so nothing changes on screen for them; what changes is
/// that a category somebody types with a capital keeps it today, through
/// `Cache.addTag`, and will not once this backend is the one storing it. `parsing::
/// capitalise` exists and is deliberately not used here.
///
/// **The old GRDB cache is left alone.** This writes nothing to it. Until a screen has
/// actually run on this backend, the cache is the working app's memory and the thing to
/// fall back to if the switch does not hold -- so during the transition it is read by
/// the old path and written by nobody through this one. That is why this opens
/// `device.sqlite` beside it rather than adopting `cache.sqlite`.
///
/// ## What is not here
///
/// `Accounts` and `Sharing`. Deliberately, and they are separate protocols for exactly
/// this reason -- a device has no account to describe and a share link names a server.
/// Those are not things to implement badly here; they are things a screen should not
/// offer, which is what the screens already do.
///
/// ## Threads
///
/// `Local` is safe to use from several threads -- sqlx's pool is built for it -- so
/// this is an actor only to give the async surface somewhere to live, not to serialise
/// anything.
actor LocalBackend {

    private let handle: OpaquePointer

    /// Opens the device's database, or answers nil if it cannot be opened.
    ///
    /// Nil rather than a throwing initialiser because there is nothing a caller can do
    /// about it: a disk that will not hold a database is not a state this app has a
    /// screen for, and the alternative is falling back to the old cache.
    init?(at location: URL = LocalServer.location) {
        guard let opened = location.path.withCString({ embedded_open($0) }) else { return nil }
        handle = opened
    }

    deinit {
        embedded_close(handle)
    }

    /// The device's backend, ready to be used, having taken over from the old cache if
    /// it had to.
    ///
    /// The one entry point a composition root should call. Opening is not enough on a
    /// device that has been used: its lists are in `cache.sqlite`, this reads
    /// `device.sqlite`, and handing the second to a screen would show an empty app with
    /// somebody's shopping still on disk.
    ///
    /// Nothing is deleted. The old cache is left exactly as it was, which is what makes
    /// this reversible: if the new path turns out to be wrong, the fallback is still
    /// sitting there with everything in it.
    ///
    /// Nil means stay on the old path -- the database would not open, or the migration
    /// refused. Both are the same instruction to a caller: use what worked yesterday.
    static func readyForUse(cache: Cache = .shared) -> LocalBackend? {
        guard let backend = LocalBackend() else { return nil }
        guard !hasTakenOver else { return backend }

        let waiting = cache.lists()
        guard !waiting.isEmpty else {
            // Nothing to bring, so nothing to get wrong. Marked all the same, so a list
            // made here tomorrow is not mistaken for a cache that needs migrating.
            hasTakenOver = true
            return backend
        }

        guard backend.bringAcross(waiting, from: cache) else { return nil }
        hasTakenOver = true
        return backend
    }

    /// Puts what this device holds where a server can be told about it.
    ///
    /// The mirror of ``readyForUse(cache:)``, and the half that was missing. That one
    /// carries the old cache *into* `device.sqlite` when somebody stops using a server;
    /// this carries `device.sqlite` back *out* when somebody adopts one. Without it,
    /// choosing a server showed an empty account with everything still on disk —
    /// `handOverIfNeeded` walks the cache for lists no server has heard of, and on a
    /// device that has only ever answered for itself the cache is empty.
    ///
    /// Nothing is deleted, for the same reason as the other direction: `device.sqlite`
    /// is left exactly as it was, so stopping the server again brings it all back. That
    /// is not a nicety — it is what makes this safe to run before anybody has proved the
    /// server works.
    ///
    /// Returns false if the device's database will not open, which a caller should treat
    /// as "do not adopt the server yet": going ahead would show an empty account, which
    /// is the bug this exists to fix.
    ///
    /// - Note: Filing is carried across by tag *id*, and those are this device's. They
    ///   agree with a server's for the categories both seeded from `reference.json`, and
    ///   do not for one somebody made here — the server refuses a tag id it does not
    ///   know, the drain forgets it, and the item arrives unfiled. Fixing that properly
    ///   means naming tags on the wire rather than numbering them.
    static func handOverToAServer(cache: Cache = .shared) -> Bool {
        guard !hasHandedOver else { return true }

        // Nothing of this device's own to hand over. A device that never took its cache
        // over is a device where the cache *is* the store, and `handOverIfNeeded`
        // already walks it -- reading `device.sqlite` here would find an empty database
        // and taking it in would be a no-op with a flag set for no reason.
        guard hasTakenOver else {
            hasHandedOver = true
            return true
        }

        guard let backend = LocalBackend() else { return false }
        guard let taken = backend.everythingHere() else { return false }

        // The copy left behind by the takeover goes first. It is the same shopping
        // under different uuids, and queueing both would tell the server about it
        // twice -- see `Cache.forgetLocalLists`.
        cache.forgetLocalLists()
        cache.takeIn(taken)
        hasHandedOver = true
        return true
    }

    /// Whether this device has already been handed to a server.
    ///
    /// Its own flag rather than `device.tookOver` reversed: the two are different
    /// journeys and a device can make both, in either order, more than once. Somebody
    /// who adopts a server, leaves it, and adopts another has handed over twice — and
    /// what the second handover carries is whatever `device.sqlite` holds by then.
    private static var hasHandedOver: Bool {
        get { UserDefaults.standard.bool(forKey: "device.handedOver") }
        set { UserDefaults.standard.set(newValue, forKey: "device.handedOver") }
    }

    /// Lets a device that has left one server be handed to the next.
    ///
    /// Called when a server is given up, because at that moment `device.sqlite` becomes
    /// the truth again and anything added to it afterwards has never been sent anywhere.
    static func mayHandOverAgain() {
        hasHandedOver = false
    }

    /// Every list on this device with its rows, read straight through the FFI.
    ///
    /// `nonisolated` and synchronous for the same reason as `bringAcross`: this runs
    /// once, at a composition root, before anything `await`s and before a screen exists
    /// to show a half-done job.
    nonisolated func everythingHere() -> [(list: List, items: [Item])]? {
        guard let lists: [List] = try? decoded(embedded_lists(handle)) else { return nil }

        var taken: [(list: List, items: [Item])] = []
        for list in lists {
            guard let rows: [Item] = try? decoded(embedded_items(handle, list.id)) else {
                // One list that will not read is not a reason to hand over the others
                // and quietly lose this one. Better to do nothing and stay standalone.
                return nil
            }
            taken.append((list, rows))
        }
        return taken
    }

    /// `answer`, for a caller that is not on the actor.
    nonisolated private func decoded<T: Decodable>(
        _ raw: UnsafeMutablePointer<CChar>?
    ) throws -> T {
        guard let raw else { throw APIError.transport(NoLocalServer()) }
        defer { embedded_free(raw) }

        let data = Data(String(cString: raw).utf8)
        let envelope = try Self.decoder.decode(Envelope<T>.self, from: data)

        if let said = envelope.error { throw APIError.badInput(said) }
        guard let value = envelope.ok else { throw APIError.transport(NoLocalServer()) }
        return value
    }

    /// Whether this device has already handed its cache over.
    ///
    /// A flag rather than "is the new database empty", because those differ in the case
    /// that matters: somebody who migrates and then deletes every list would be migrated
    /// again on the next launch, and their deleted lists would come back.
    private static var hasTakenOver: Bool {
        get { UserDefaults.standard.bool(forKey: "device.tookOver") }
        set { UserDefaults.standard.set(newValue, forKey: "device.tookOver") }
    }

    /// Hands the old cache's contents to the device's server.
    ///
    /// Synchronous, and deliberately so: this runs once, before a screen is built, and
    /// a half-migrated app is worse than a pause. Everything goes through `domain`'s
    /// services on the other side, so what arrives has been through the same rules as
    /// anything added today -- including recording each use, which rebuilds the history
    /// a device had spent months collecting.
    /// `nonisolated` because it touches nothing this actor protects except the handle,
    /// which `Local` is itself safe to use from several threads -- see `web/embedded`.
    /// It also has to be callable before anything `await`s, which is the whole point of
    /// doing this before a screen exists.
    nonisolated func bringAcross(_ lists: [List], from cache: Cache) -> Bool {
        let payload: [String: Any] = [
            "lists": lists.map { list in
                [
                    "name": list.name,
                    "items": cache.items(on: list).map { item in
                        [
                            "uuid": item.uuid,
                            "name": item.name,
                            "amount": item.amount,
                            "unit_id": item.unitID as Any,
                            "done_at": item.doneAt.map { Int64($0.timeIntervalSince1970) } as Any,
                            "tag_ids": item.tagIDs,
                        ] as [String: Any]
                    },
                ] as [String: Any]
            },
        ]

        guard let json = try? JSONSerialization.data(withJSONObject: payload),
              let text = String(data: json, encoding: .utf8)
        else { return false }

        return text.withCString { text in
            guard let raw = embedded_import(handle, text) else { return false }
            defer { embedded_free(raw) }
            // Only the absence of an error matters here: how many rows came across is
            // the migration's business, and a caller that has none is a caller with an
            // empty cache.
            let answer = String(cString: raw)
            return !answer.contains("\"error\"")
        }
    }


    // MARK: - Reading

    func lists() async throws -> Listing<List> {
        let rows: [List] = try answer(embedded_lists(handle))
        // No paging: a device reading its own file has no reason to withhold the second
        // hundred, so nothing is ever truncated. The type is the server's all the same,
        // because the screens read `truncated` and should not care which backend they
        // are talking to.
        return Listing(items: rows, total: Int64(rows.count), truncated: false)
    }

    func items(on list: List) async throws -> Listing<Item> {
        let rows: [Item] = try answer(embedded_items(handle, list.id))
        return Listing(items: rows, total: Int64(rows.count), truncated: false)
    }

    func units() async throws -> [Unit] {
        try answer(embedded_units(handle))
    }

    func tags(orderedFor list: List) async throws -> [Tag] {
        try answer(embedded_tags(handle, list.id))
    }

    func tags(on item: Item, in list: List) async throws -> [Tag] {
        try answer(embedded_tags_on(handle, item.id))
    }

    func suggestions(matching typed: String, on list: List) async throws -> [String] {
        try typed.withCString { query in
            try answer(embedded_suggestions(handle, list.id, query))
        }
    }

    func history(on list: List) async throws -> [RememberedEntry] {
        try answer(embedded_history(handle, list.id))
    }

    // MARK: - Lists

    func createList(named name: String) async throws -> List {
        try name.withCString { name in try answer(embedded_make_list(handle, name)) }
    }

    func rename(_ list: List, to name: String) async throws {
        try name.withCString { name in
            try nothing(embedded_rename_list(handle, list.id, name))
        }
    }

    func delete(_ list: List) async throws {
        try nothing(embedded_delete_list(handle, list.id))
    }

    // MARK: - What is on one

    func add(_ line: String, to list: List) async throws {
        try line.withCString { line in
            // No uuid: this device has not drawn the row yet, so the one the database
            // mints is the one it will be known by everywhere.
            try nothing(embedded_add(handle, list.id, line, nil))
        }
    }

    func setDone(_ item: Item, on list: List, done: Bool, at: Date) async throws {
        try nothing(embedded_set_done(handle, item.id, done, Int64(at.timeIntervalSince1970)))
    }

    func setDone(itemID: Int64, listID: Int64, done: Bool) async throws {
        // Zero for now: this form is for a caller holding a queued operation rather than
        // a row, and the one that carries a time uses the other.
        try nothing(embedded_set_done(handle, itemID, done, 0))
    }

    func update(
        _ item: Item,
        on list: List,
        name: String,
        amount: Double,
        unitID: Int64?
    ) async throws {
        try name.withCString { name in
            // Zero for none: C has no optional, and the units are counted from one.
            try nothing(embedded_update_item(handle, item.id, name, amount, unitID ?? 0))
        }
    }

    func attach(_ tag: Tag, to item: Item, on list: List) async throws {
        try nothing(embedded_attach_tag(handle, item.id, tag.id))
    }

    func detach(_ tag: Tag, from item: Item, on list: List) async throws {
        try nothing(embedded_detach_tag(handle, item.id, tag.id))
    }

    func clearDone(on list: List) async throws {
        try nothing(embedded_clear_done(handle, list.id))
    }

    func delete(_ item: Item, on list: List) async throws {
        try nothing(embedded_delete_item(handle, item.id))
    }

    // MARK: - The categories

    func setTagOrder(_ tags: [Tag], on list: List) async throws {
        // One array rather than one call per row: the order is a single fact about the
        // list, and applying it row by row would leave it half-applied if anything
        // failed halfway.
        let ids = tags.map(\.id)
        let json = String(data: try JSONEncoder().encode(ids), encoding: .utf8) ?? "[]"
        try json.withCString { json in
            try nothing(embedded_set_tag_order(handle, list.id, json))
        announceCategories()
        }
    }

    func createTag(named name: String, emoji: String?) async throws -> Tag {
        try name.withCString { name in
            try withOptionalCString(emoji) { emoji in
                let made: Tag = try answer(embedded_create_tag(handle, name, emoji))
                announceCategories()
                return made
            }
        }
    }

    func updateTag(_ tag: Tag, named name: String, emoji: String?) async throws -> Tag {
        try name.withCString { name in
            try withOptionalCString(emoji) { emoji in
                let renamed: Tag = try answer(embedded_update_tag(handle, tag.id, name, emoji))
                announceCategories()
                return renamed
            }
        }
    }

    func deleteTag(_ tag: Tag) async throws {
        try nothing(embedded_delete_tag(handle, tag.id))
        announceCategories()
    }

    // MARK: - Somebody else changed something

    /// Which lists this person can see, whenever that changes.
    ///
    /// The same shape the API answers with, so a screen watching one cannot tell which
    /// it has -- but underneath it is `domain`'s own broadcast channel, the one the
    /// server drives SSE from. Standalone and server mode are told about changes by
    /// literally the same mechanism.
    func listChanges() async throws -> AsyncThrowingStream<Void, Error> {
        try watching { embedded_watch_lists($0) }
    }

    /// This list, and the categories it is walked by.
    ///
    /// Two sources, because `domain` only announces one of them. `service::tags::attach`
    /// and `detach` announce on the list's channel -- those are rows. Creating,
    /// renaming, removing or reordering a category announces **nothing**, because a
    /// category belongs to no list and there is no channel for "the vocabulary moved".
    ///
    /// So this says it itself: every tag mutation on this backend tells whoever is
    /// watching. Without that, renaming a category in Settings would not reach an open
    /// list on a device answering for itself -- the same bug that was fixed on the
    /// cached path this morning, arriving by a different road.
    func changes(on list: List) async throws -> AsyncThrowingStream<Nudge, Error> {
        let rows = try watching { embedded_watch_list($0, list.id) }
        let (stream, continuation) = AsyncThrowingStream<Nudge, Error>.makeStream()

        // Registered here, synchronously, on the actor -- not from inside a stream's
        // build closure through a `Task`. That version lost the race with a category
        // edited immediately after the watch began, which is precisely the case: a
        // caller starts watching and then changes something.
        let token = UUID()
        categoryWatchers[token] = continuation

        let fromRows = Task {
            for try await _ in rows { continuation.yield(.rows) }
            continuation.finish()
        }
        continuation.onTermination = { [weak self] _ in
            fromRows.cancel()
            Task { await self?.removeCategoryWatcher(token) }
        }

        return stream
    }

    /// Whoever is watching a list wants to know the vocabulary moved.
    ///
    /// Keyed so a screen that goes away stops being told; an unbounded, never-pruned
    /// list of continuations is a leak that only shows up after somebody has opened
    /// forty lists.
    private var categoryWatchers: [UUID: AsyncThrowingStream<Nudge, Error>.Continuation] = [:]

    private func removeCategoryWatcher(_ token: UUID) {
        categoryWatchers[token] = nil
    }

    /// Says the vocabulary moved, to every list that is open.
    private func announceCategories() {
        for watcher in categoryWatchers.values { watcher.yield(.categories) }
    }

    /// Turns a blocking Rust watcher into a stream.
    ///
    /// A thread of its own, because `embedded_next_change` parks until something
    /// happens -- which on a shopping list is most of the time. It cannot be a `Task`:
    /// a blocked task holds a cooperative thread pool thread, and enough of those
    /// starve everything else in the app.
    ///
    /// The ownership dance matters and is the one thing the C header warns about.
    /// Freeing a watcher another thread is parked in is a use-after-free, so the
    /// watching thread owns it and frees it only after `next_change` has returned nil.
    /// Cancelling the stream stops it, which is what makes that return.
    private func watching(
        _ start: (OpaquePointer) -> OpaquePointer?
    ) throws -> AsyncThrowingStream<Void, Error> {
        guard let watcher = start(handle) else {
            throw APIError.transport(NoLocalServer())
        }
        guard let stopper = embedded_watcher_stopper(watcher) else {
            embedded_watcher_free(watcher)
            throw APIError.transport(NoLocalServer())
        }

        return AsyncThrowingStream { continuation in
            // `nonisolated(unsafe)` on the pointers: they are handed to exactly one
            // thread, which is the only thing that touches the watcher, and to one
            // termination handler, which is the only thing that touches the stopper.
            nonisolated(unsafe) let parked = watcher
            nonisolated(unsafe) let ending = stopper

            let thread = Thread {
                while let answer = embedded_next_change(parked) {
                    // The answer says *what* changed. A caller of this protocol asked
                    // to be told that something did, and re-reads -- the same nudge the
                    // server's stream carries, and for the same reason: a watcher given
                    // the rows becomes a second opinion about them.
                    embedded_free(answer)
                    continuation.yield()
                }
                // nil means stopped, and only then is nobody parked in it.
                embedded_watcher_free(parked)
                continuation.finish()
            }
            thread.name = "shoppinglist.local.watch"
            thread.start()

            continuation.onTermination = { _ in
                embedded_stop(ending)
                embedded_stopper_free(ending)
            }
        }
    }

    // MARK: - The boundary

    /// Decodes `{"ok": …}`, or throws what `{"error": …}` said.
    ///
    /// Takes ownership of the string, so every path frees it exactly once -- including
    /// the throwing ones, which is where a leak would otherwise live.
    private func answer<T: Decodable>(_ raw: UnsafeMutablePointer<CChar>?) throws -> T {
        guard let raw else { throw APIError.transport(NoLocalServer()) }
        defer { embedded_free(raw) }

        let data = Data(String(cString: raw).utf8)
        let envelope = try Self.decoder.decode(Envelope<T>.self, from: data)

        if let said = envelope.error { throw APIError.badInput(said) }
        guard let value = envelope.ok else { throw APIError.transport(NoLocalServer()) }
        return value
    }

    /// The same, for a call whose answer is only whether it worked.
    private func nothing(_ raw: UnsafeMutablePointer<CChar>?) throws {
        guard let raw else { throw APIError.transport(NoLocalServer()) }
        defer { embedded_free(raw) }

        let data = Data(String(cString: raw).utf8)
        let envelope = try Self.decoder.decode(Envelope<Ignored>.self, from: data)
        if let said = envelope.error { throw APIError.badInput(said) }
    }

    private struct Envelope<T: Decodable>: Decodable {
        let ok: T?
        let error: String?
    }

    /// For the calls whose `ok` is a row nobody reads back.
    private struct Ignored: Decodable {
        init(from decoder: Decoder) throws {}
    }

    /// The same reader the API uses, because it is reading the same wire.
    private static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()

    /// `withCString` for something that may be absent.
    private func withOptionalCString<T>(
        _ value: String?,
        _ body: (UnsafePointer<CChar>?) throws -> T
    ) rethrows -> T {
        guard let value else { return try body(nil) }
        return try value.withCString(body)
    }
}

/// The device's own database could not answer.
///
/// A transport failure rather than a category of its own: the screens already know what
/// to do with one, and a disk that will not cooperate is as much "cannot reach it" as a
/// network that will not.
struct NoLocalServer: Error {}

extension LocalBackend: Backend {}

#endif
