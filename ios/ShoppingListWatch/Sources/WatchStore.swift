import Foundation
import Observation
import WatchConnectivity

/// What this watch is, and who it talks to.
///
/// The watch is a full client either way — its own cache, its own outbox, usable with
/// the phone left at home. This decides only where that queue drains to, and it takes
/// the answer from the phone rather than guessing: there is one place that knows
/// whether this household has a server, and it is not the wrist.
///
/// See `WatchLink` for the whole arrangement.
@MainActor
@Observable
final class WatchStore: NSObject, WCSessionDelegate {

    enum Mode: Equatable {
        /// Nothing has been heard from the phone yet. Not the same as having no server:
        /// a watch that has never been told anything must not decide it is alone and
        /// start behaving as though the phone's lists do not exist.
        case unknown
        /// A server, at this address. The watch talks to it directly.
        case server(ServerAddress)
        /// No server anywhere. The phone holds the lists and is the far end.
        case onDevice
    }

    private(set) var mode: Mode = .unknown

    /// Ticks made here that have not reached wherever they are going.
    private(set) var waiting = 0

    /// Whether the phone has ever said anything at all.
    ///
    /// What earns an empty state. A watch that has heard nothing is not a watch whose
    /// owner has no lists, and saying "no lists" to somebody who has ten is worse than
    /// saying nothing.
    private(set) var heard = false

    private let cache = Cache.shared

    /// The credential, when there is a server. Held here rather than in the views so
    /// that a token fetched for one screen is not fetched again for the next.
    private let identity = WatchIdentity()

    override init() {
        super.init()
        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()
        // Whatever arrived while this app was not running. Read rather than waited for:
        // the system holds the last context and does not re-deliver it on launch.
        adopt(WCSession.default.receivedApplicationContext)
        refreshWaiting()
    }

    // MARK: - Where the queue goes

    /// The far end, or nothing if this watch does not yet know what it is.
    ///
    /// One queue, two destinations — the whole point of `Destination`. Everything the
    /// drain does with the answers is identical; only the address of the other end
    /// differs.
    var destination: Destination? {
        switch mode {
        case .unknown:
            return nil
        case .onDevice:
            return PhoneDestination()
        case .server(let address):
            return API(server: { address.url }, token: { [identity] in await identity.token() })
        }
    }

    /// Whether this watch can fetch a list for itself.
    ///
    /// With a server it can, and does. With none it cannot and must not try: the lists
    /// arrive from the phone, and a screen that "loads" would only ever be able to fail.
    var fetches: Bool {
        if case .server = mode { return true }
        return false
    }

    /// Empties the queue, wherever its contents belong.
    @discardableResult
    func send() async -> Drained {
        guard let destination, cache.outbox.waiting > 0 else {
            refreshWaiting()
            return Drained()
        }
        let drained = await cache.outbox.drain(through: destination)
        refreshWaiting()
        return drained
    }

    /// The server said no to the credential we had.
    ///
    /// The only reliable news that a session has ended — revoked on the phone, or idle
    /// past ninety days. Throwing it away makes the next request ask the phone for a
    /// new one, which is the whole recovery path on a watch.
    func credentialRefused() {
        identity.refused()
    }

    func refreshWaiting() {
        waiting = cache.outbox.waiting
    }

    // MARK: - What the phone says

    private func adopt(_ context: [String: Any]) {
        guard let snapshot = WatchLink.decode(context, as: WatchLink.Snapshot.self) else {
            return
        }
        heard = true

        if snapshot.onDeviceOnly {
            mode = .onDevice
        } else if let raw = snapshot.server,
                  case .success(let address) = ServerAddress.parse(raw) {
            let was = mode
            mode = .server(address)
            // A different server is a different world: its ids, its uuids, its people.
            // Keeping the old one's rows would show one server's shopping under
            // another's name -- the same rule the phones follow when the address
            // changes (C4).
            if case .server(let before) = was, before != address {
                cache.forgetEverything()
                identity.refused()
            }
        }

        // Only ever sent when there is no server, because only then is the phone the
        // authority on what a list contains. With a server the watch asks it directly
        // and a second opinion here would be a second source of truth.
        if snapshot.onDeviceOnly {
            write(snapshot)
        }
    }

    /// Writes the phone's picture into this watch's own cache.
    ///
    /// Into the cache and not into a variable, which is what makes the watch usable
    /// with the phone at home: the rows are still there at the next launch with nothing
    /// running and nothing in range. The queue sits on top of them exactly as it does
    /// on the phones — see `Outbox`.
    private func write(_ snapshot: WatchLink.Snapshot) {
        cache.remember(units: snapshot.units.map { Unit(id: $0.id, name: $0.name) })

        let lists = snapshot.lists.enumerated().map { at, list in
            // A negative id, because there is no server to have minted a real one and
            // the uuid is the only name that means anything here. Stable across
            // snapshots so that navigation does not break under somebody's thumb.
            List(
                id: -Int64(at + 1),
                uuid: list.id,
                name: list.name,
                ownerID: 0,
                role: .editor
            )
        }
        cache.remember(lists: lists)

        for (list, wire) in zip(lists, snapshot.lists) {
            cache.remember(
                items: wire.items.enumerated().map { at, item in
                    Item(
                        id: -Int64(at + 1),
                        uuid: item.id,
                        name: item.name,
                        amount: item.amount,
                        unitID: item.unitID,
                        doneAt: item.done ? Date() : nil,
                        tagIDs: item.tagIDs
                    )
                },
                on: list
            )
            cache.remember(
                tags: wire.tags.map {
                    Tag(id: $0.id, name: $0.name, emoji: $0.emoji, sortOrder: $0.sortOrder)
                },
                on: list
            )
        }
    }

    // MARK: - Session

    nonisolated func session(
        _ session: WCSession,
        activationDidCompleteWith state: WCSessionActivationState,
        error: Error?
    ) {
        Task { @MainActor in
            adopt(WCSession.default.receivedApplicationContext)
            await send()
        }
    }

    nonisolated func session(
        _ session: WCSession,
        didReceiveApplicationContext applicationContext: [String: Any]
    ) {
        Task { @MainActor in
            adopt(applicationContext)
            // Back in range, so anything queued can go now. This is what "aligned when
            // they are back together" actually is: the watch sends what it did, the
            // phone applies it, and the phone's next snapshot already agrees.
            await send()
        }
    }

    nonisolated func sessionReachabilityDidChange(_ session: WCSession) {
        Task { @MainActor in await send() }
    }
}
