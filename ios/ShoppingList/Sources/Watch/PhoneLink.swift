import Foundation
import WatchConnectivity

/// The phone's half of the link to the watch.
///
/// The phone **is** the watch's server — see `WatchLink` for why, and for what that
/// costs. Two jobs, and they are the only two: push a picture of the lists when it
/// changes, and take back the ticks made on the wrist.
///
/// Everything about a list that is hard — merging, queueing, deciding whether there is
/// a server at all — stays here, where it is already written and already tested. What
/// crosses to the watch is a flat picture with the units spelled out and the rows in
/// the order the shop is walked.
@MainActor
final class PhoneLink: NSObject, WCSessionDelegate {
    static let shared = PhoneLink()

    private let cache = Cache.shared
    /// What was last handed over, so an unchanged snapshot is not sent again.
    ///
    /// `updateApplicationContext` refuses a payload identical to the last one anyway,
    /// but it refuses it by throwing, and a thrown error that means "nothing to do" is
    /// noise in a log that should only carry real ones.
    private var lastSent: WatchLink.Snapshot?

    /// A current credential for the server, when there is one.
    ///
    /// Set by the app at startup. A closure rather than a direct call, because what
    /// counts as current is the identity's business and it may have to go and get one.
    var token: (() async -> String?)?

    private override init() {
        super.init()
    }

    func start() {
        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()

        // Every change to what this device holds, from anywhere in the app. Listening
        // rather than being called means nobody adding a screen has to remember the
        // watch exists.
        NotificationCenter.default.addObserver(
            forName: .cacheChanged,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.pushSoon() }
        }

        // A server appearing or going away changes what the dot on the wrist means.
        NotificationCenter.default.addObserver(
            forName: .serverChanged,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.pushSoon() }
        }
    }

    /// Pushes once, after the current burst of changes.
    ///
    /// Loading a list writes the lists and then the items, and a screenful of ticks
    /// writes once each. Sending a snapshot per write would spend the link on states
    /// nobody will ever see; the watch only wants the one at the end.
    private func pushSoon() {
        pending?.cancel()
        pending = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            push()
        }
    }

    private var pending: Task<Void, Never>?

    // MARK: - Sending

    /// Hands the watch the current state of things.
    ///
    /// Cheap and idempotent, so callers do not have to work out whether it is needed:
    /// call it whenever a list might have changed. Nothing happens if the answer is
    /// what the watch already has.
    func push() {
        let session = WCSession.default
        guard session.activationState == .activated else { return }
        // Not `isReachable`: an application context is delivered whenever the system
        // next can, including with both apps closed. Checking reachability here would
        // skip exactly the case this mechanism exists for.
        guard session.isPaired, session.isWatchAppInstalled else { return }

        let snapshot = snapshot()
        guard snapshot != lastSent else { return }

        do {
            try session.updateApplicationContext(WatchLink.encode(snapshot))
            lastSent = snapshot
        } catch {
            // Nothing to do about it and nobody to tell: the watch keeps the last
            // picture it had, which is the correct behaviour for a failed update.
            // The next change tries again.
            print("[watch] could not hand over the lists: \(error)")
        }
    }

    /// What the watch should be showing.
    ///
    /// Read from the cache rather than the network, deliberately. The cache is what the
    /// phone itself shows, so the watch agrees with the phone even when both are out of
    /// signal — and on a device with no server the cache is not a copy of anything, it
    /// is the lists.
    private func snapshot() -> WatchLink.Snapshot {
        var remaining = WatchLink.cap
        let units = cache.units()

        let lists = cache.lists().map { list -> WatchLink.ListOnTheWatch in
            let tags = cache.tags(on: list)
            let all = cache.items(on: list)

            // In the order the shop is walked, before the cap, so what is dropped is
            // the tail of the walk rather than an arbitrary slice of it. Crossed-off
            // rows come last: they are the least useful thing to spend the budget on.
            let outstanding = grouped(all.filter { !$0.isDone }, by: tags).flatMap(\.items)
            let done = all.filter(\.isDone)
            let ordered = outstanding + done

            let sent = ordered.prefix(remaining)
            remaining -= sent.count

            return WatchLink.ListOnTheWatch(
                id: list.uuid,
                name: list.name,
                tags: tags.map {
                    WatchLink.TagOnTheWatch(
                        id: $0.id,
                        name: $0.name,
                        emoji: $0.emoji,
                        sortOrder: $0.sortOrder
                    )
                },
                items: sent.map { item in
                    WatchLink.ItemOnTheWatch(
                        id: item.uuid,
                        name: item.name,
                        amount: item.amount,
                        unitID: item.unitID,
                        done: item.isDone,
                        tagIDs: item.tagIDs
                    )
                },
                total: all.count,
                truncated: sent.count < ordered.count
            )
        }

        return WatchLink.Snapshot(
            lists: lists,
            onDeviceOnly: ServerDirectory.isOnDeviceOnly,
            server: ServerDirectory.current?.origin,
            units: units.map { WatchLink.UnitOnTheWatch(id: $0.id, name: $0.name) }
        )
    }

    // MARK: - Receiving

    /// The watch's queue, arriving the immediate way — it is in range and awake.
    ///
    /// Answered rather than acknowledged: the watch is running `Outbox.drain` and is
    /// waiting to be told what to forget.
    nonisolated func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any],
        replyHandler: @escaping ([String: Any]) -> Void
    ) {
        // The credential, asked for rather than pushed. First, because it is the one
        // message that arrives before the watch knows anything else.
        if message[WatchLink.tokenRequest] != nil {
            Task { @MainActor in
                // An empty reply rather than none, for the same reason as below.
                guard let token = await token?(), let server = ServerDirectory.current else {
                    replyHandler([:])
                    return
                }
                replyHandler([
                    WatchLink.tokenRequest: token,
                    WatchLink.serverAddress: server.origin,
                ])
            }
            return
        }

        guard let request = WatchLink.decode(message, as: WatchLink.SyncRequest.self) else {
            // A watch asking for something this build does not understand. An empty
            // answer rather than none: it is waiting, and a reply that never comes
            // leaves it hanging until WatchConnectivity times out.
            replyHandler([:])
            return
        }

        Task { @MainActor in
            let outcomes = await WatchTicks.replay(request.operations)
            replyHandler(WatchLink.encode(WatchLink.SyncReply(outcomes: outcomes)))
            // The watch's picture is now out of date by exactly the changes it just
            // sent. It has already drawn them, so this is not what makes it look
            // right — it is what stops the next snapshot from arriving without them
            // and undoing them on screen.
            push()
        }
    }

    /// The credential, when there is a server.
    ///
    /// Request and reply rather than pushed: a session token is not something to leave
    /// lying in an application context, which is persisted and latest-wins. The watch
    /// asks when it needs one — see `WatchIdentity`.
    nonisolated func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any]
    ) {
        // The no-reply form. Nothing the watch sends today uses it, but a delegate
        // that implements only the other leaves such a sender waiting on a timeout.
    }

    // MARK: - Session lifecycle

    nonisolated func session(
        _ session: WCSession,
        activationDidCompleteWith state: WCSessionActivationState,
        error: Error?
    ) {
        // A watch that has just been paired, or an app that has just been installed on
        // one, has never been told anything.
        Task { @MainActor in push() }
    }

    nonisolated func sessionWatchStateDidChange(_ session: WCSession) {
        Task { @MainActor in push() }
    }

    // Required on iOS, and both are ordinary: switching watches, or taking one off.
    nonisolated func sessionDidBecomeInactive(_ session: WCSession) {}

    nonisolated func sessionDidDeactivate(_ session: WCSession) {
        // Re-activating is what picks up the newly paired watch.
        WCSession.default.activate()
    }
}
