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

    func setDone(_ item: Item, on list: List, done: Bool) async throws {
        try nothing(embedded_set_done(handle, item.id, done))
    }

    func setDone(itemID: Int64, listID: Int64, done: Bool) async throws {
        try nothing(embedded_set_done(handle, itemID, done))
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
        }
    }

    func createTag(named name: String, emoji: String?) async throws -> Tag {
        try name.withCString { name in
            try withOptionalCString(emoji) { emoji in
                try answer(embedded_create_tag(handle, name, emoji))
            }
        }
    }

    func updateTag(_ tag: Tag, named name: String, emoji: String?) async throws -> Tag {
        try name.withCString { name in
            try withOptionalCString(emoji) { emoji in
                try answer(embedded_update_tag(handle, tag.id, name, emoji))
            }
        }
    }

    func deleteTag(_ tag: Tag) async throws {
        try nothing(embedded_delete_tag(handle, tag.id))
    }

    // MARK: - Somebody else changed something

    /// Never yields, and that is the right answer rather than a gap.
    ///
    /// This stream is for changes made *elsewhere*. On a device there is no elsewhere:
    /// every change is made here, and the screen hears about it from the database. A
    /// conformer that failed instead would put an error path back on a screen that has
    /// nothing wrong with it, which is the whole problem being solved.
    ///
    /// `web/embedded` does have a watcher, and it is what will drive the screens once
    /// they read through this backend. It is not wired to this stream yet because
    /// nothing reads from it yet.
    func listChanges() async throws -> AsyncThrowingStream<Void, Error> {
        AsyncThrowingStream { _ in }
    }

    func changes(on list: List) async throws -> AsyncThrowingStream<Void, Error> {
        AsyncThrowingStream { _ in }
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
