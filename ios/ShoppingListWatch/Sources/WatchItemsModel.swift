import Foundation
import Observation

/// What is on one list, on a wrist.
///
/// Out of the view, for the reason `ItemsModel` and `ListsModel` are: this was three
/// hundred and seventy-seven lines of logic inside a SwiftUI `View`, which is how the
/// phone's copy came to drift from the Mac's in four places in a single afternoon, and
/// why none of it could be tested without hosting a screen. `docs/review.md` item one,
/// for the last client that still had the problem.
///
/// **Not a `Backend`, and that is a toolchain limit rather than a choice.** A backend
/// needs `QuickAdd` -- the Rust parser's Swift face -- and a watchOS *device* build asks
/// for `arm64` and `arm64_32`, which Rust has no stable target for: `arm64_32-apple-watchos`
/// is tier three and needs `-Z build-std` on nightly. The simulator target exists and
/// builds. Until the device one does, this keeps the cache and the queue directly, and
/// is the only thing in the app that still does.
///
/// What it drains through is a `Destination`, which is either the server or the phone --
/// see `WatchStore.send`. That much was already shared.
@MainActor
@Observable
final class WatchItemsModel {
    let list: List

    /// The server, when there is one. With none this is nil and every path that would
    /// have asked it is skipped -- the phone puts the list in the cache instead.
    private let api: API?
    private let cache: Cache
    private let store: WatchStore

    var items: [Item] = []
    var truncated = false
    var total: Int64 = 0
    var units: [Int64: String] = [:]
    var tags: [Tag] = []
    var problem: Problem?
    var loaded = false
    /// Rows waiting for the server. Kept so a tap looks instant on a wrist, where the
    /// round trip is the phone's connection plus the server's.
    var inFlight: Set<Int64> = []
    /// Ticks made here that have not been sent -- see the phone's `ItemsModel`.
    var unsent: Set<String> = []
    var waiting = 0
    var fresh = false
    private var draining = false

    init(list: List, store: WatchStore, cache: Cache = .shared) {
        self.list = list
        self.store = store
        self.api = store.destination as? API
        self.cache = cache
    }

    /// The phone pushed a new picture into the cache.
    ///
    /// Only with no server, where this is how a change arrives at all -- with one, the
    /// change stream says so. In the view this reached for `api` and `cache` directly,
    /// which is the last thing that kept those two names in a SwiftUI file.
    func cacheChanged() {
        guard api == nil else { return }
        items = withUnsent(cache.items(on: list))
        refreshUnsent()
    }

    var outstanding: [Item] { items.filter { !$0.isDone } }

    /// Outstanding items in the order the shop is laid out, with no headings.
    ///
    /// The same grouping the phone and the browser use, flattened. Tags earn their
    /// place here by putting the right things next to each other; a heading over each
    /// run would cost a row apiece to say what the order already says, on a screen with
    /// six of them.
    var ordered: [Item] { grouped(outstanding, by: tags).flatMap(\.items) }
    var done: [Item] { items.filter(\.isDone) }



    func load() async {
        // No server means nothing to ask. The list is in the cache because the phone
        // put it there, and a request here could only ever fail -- which would put an
        // error on screen about a server nobody has.
        guard let api else {
            items = withUnsent(cache.items(on: list))
            total = Int64(items.count)
            fresh = store.heard
            loaded = true
            await drain()
            return
        }
        do {
            let listing = try await api.items(on: list)
            cache.remember(items: listing.items, on: list)
            self.items = withUnsent(listing.items)
            self.truncated = listing.truncated
            self.total = listing.total
            problem = nil
            fresh = true
            loaded = true
            await drain()
        } catch {
            problem = Problem(error)
        }
        loaded = true
    }


    /// Reference data, once. On a watch this matters twice over: it is the slowest
    /// connection of the three, and it is relayed through a phone.
    func loadReference() async {
        // With no server the phone has already put both in the cache -- the same
        // rows, with the same ids, that a server would have supplied.
        guard let api else {
            seedReference()
            return
        }
        do {
            async let units = api.units()
            async let tags = api.tags(orderedFor: list)
            let (loadedUnits, loadedTags) = try await (units, tags)
            self.units = Dictionary(uniqueKeysWithValues: loadedUnits.map { ($0.id, $0.name) })
            self.tags = loadedTags
        } catch {
            // Not swallowed, which is what this used to do. Failing here left `units`
            // and `tags` empty for as long as the screen was up, so every row lost its
            // measure and its aisle -- on the device with the worst connection of the
            // three, where the ask is relayed through a phone and fails most often.
            //
            // Nothing is shown, because a poorer list is not a broken one and `load()`
            // already reports what actually stops the screen working. But poorer is
            // recovered from rather than accepted: the phone syncs the same rows into
            // this cache, and what shipped with the app stands in if even that is
            // empty. Same fallback as `ItemsModel.seedReference`, same ids.
            seedReference()
        }
    }


    /// What the server would have said, from the cache or from the bundle.
    ///
    /// Only fills what is missing: an answer already on screen is a real one, and a
    /// half-failed load should not have the shipped set written over the top of it.
    func seedReference() {
        if units.isEmpty {
            let remembered = cache.units()
            let known = remembered.isEmpty ? Reference.units : remembered
            units = Dictionary(uniqueKeysWithValues: known.map { ($0.id, $0.name) })
        }
        if tags.isEmpty {
            let remembered = cache.tags(on: list)
            tags = remembered.isEmpty ? Reference.tags : remembered
        }
    }


    /// The last list this watch saw, put up before anything is asked of the server.
    func showWhatWeHave() {
        guard !fresh else { return }
        let remembered = cache.items(on: list)
        guard !remembered.isEmpty else { return }
        items = remembered
        total = Int64(remembered.count)
        loaded = true
    }


    /// The server's answer with this watch's unsent ticks laid back over it.
    func withUnsent(_ fromServer: [Item]) -> [Item] {
        let queued = cache.outbox.forList(list)
        guard !queued.isEmpty else { return fromServer }
        var rows = fromServer
        for operation in queued where operation.kind == QueuedOperation.Kind.setDone {
            rows = rows.map {
                $0.uuid == operation.itemUUID ? $0.withDone(operation.done) : $0
            }
        }
        return rows
    }



    func toggle(_ item: Item) async {
        // A viewer's tap would be refused by the server, and a row that greys out and
        // comes back unchanged is a worse answer than one that does not move.
        guard list.mayEdit else { return }

        let done = !item.isDone
        cache.outbox.setDone(item, on: list, done: done)
        items = items.map { $0.uuid == item.uuid ? $0.withDone(done) : $0 }
        cache.remember(items: items, on: list)
        refreshUnsent()

        await drain()
    }


    /// Sends what is queued. See the phone's `ItemsView.drain` — the rules are the
    /// same, and only the losses are said out loud.
    func drain() async {
        guard !draining else { return }
        // See the phone's copy: the lists screen drains the same queue, so this count
        // goes stale unless it is read back even when there is nothing to send.
        refreshUnsent()
        guard cache.outbox.waiting > 0 else { return }

        draining = true
        // Wherever this watch's queue goes -- the server, or the phone when there is
        // no server. Identical rules either way; see `Destination`.
        let drained = await store.send()
        draining = false

        refreshUnsent()
        if drained.sent > 0 { await load() }
    }


    /// Tries the queue again while anything is in it, so a tick does not wait for
    /// somebody else to touch the list.
    func keepTrying() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(10))
            await drain()
        }
    }



    func refreshUnsent() {
        let queued = cache.outbox.forList(list)
        unsent = Set(queued.map(\.itemUUID))
        waiting = queued.count
    }


    /// Keeps the wrist in step with the phone and the browser.
    ///
    /// The same stream the phone watches, through the same shared client. A watch is
    /// the screen most likely to be showing a list somebody else is changing -- the
    /// other half of the shop, holding the phone -- so it is the one that can least
    /// afford to be quietly stale.
    func watch() async {
        // Only a server has a change stream. With none, the phone tells this watch
        // what changed by pushing a new picture, which lands in the cache.
        guard let api else { return }
        var reconnecting = false

        while !Task.isCancelled {
            if reconnecting { await load() }

            do {
                for try await _ in try await api.changes(on: list) {
                    await load()
                }
            } catch let problem as APIError {
                // A stream refused for want of a token means the cached one has
                // expired. Dropping it makes the next attempt ask the phone again,
                // which is the whole recovery path on a watch.
                if case .unauthorized = problem {
                    store.credentialRefused()
                }
            } catch {
                // Anything else is the connection going away, which on a watch is a
                // lowered wrist. Ordinary, and not worth showing.
            }

            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
        }
    }

}
