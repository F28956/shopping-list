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

    /// Applies a tick that arrived from the wrist.
    ///
    /// Set by the app at startup. A closure rather than a direct call into the cache,
    /// because crossing something off is not one write: it is an optimistic change, a
    /// queued operation and a drain, and that sequence lives in one place already.
    var apply: ((WatchLink.Tick) async -> Void)?

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

        let lists = cache.lists().map { list -> WatchLink.ListOnTheWatch in
            let units = Dictionary(
                uniqueKeysWithValues: cache.units().map { ($0.id, $0.name) }
            )
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
                    WatchLink.TagOnTheWatch(id: $0.id, name: $0.name, emoji: $0.emoji)
                },
                items: sent.map { item in
                    WatchLink.ItemOnTheWatch(
                        id: item.uuid,
                        name: item.name,
                        amount: item.amount,
                        // Spelled here, so the watch needs no units and no rule about
                        // how to write one.
                        measure: item.measure(units: units),
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
            onDeviceOnly: ServerDirectory.isOnDeviceOnly
        )
    }

    // MARK: - Receiving

    /// A tick that arrived the queued way — the phone was asleep or out of range when
    /// it was made.
    nonisolated func session(
        _ session: WCSession,
        didReceiveUserInfo userInfo: [String: Any]
    ) {
        take(userInfo)
    }

    /// A tick that arrived the immediate way — the phone was awake and nearby. Same
    /// payload, same handling; only the route differs. See `WatchStore.send`.
    nonisolated func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any]
    ) {
        take(message)
    }

    /// The reply-expected form. WatchConnectivity calls whichever of the two the sender
    /// asked for, and a delegate implementing only the other leaves that sender waiting
    /// for a timeout, so both are here and both answer.
    nonisolated func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any],
        replyHandler: @escaping ([String: Any]) -> Void
    ) {
        take(message)
        replyHandler([:])
    }

    private nonisolated func take(_ payload: [String: Any]) {
        guard let tick = WatchLink.decode(payload, as: WatchLink.Tick.self) else { return }
        Task { @MainActor in
            await apply?(tick)
            // The watch's picture is now out of date by exactly the change it just
            // made. It has already drawn the row ticked, so this is not what makes it
            // look right — it is what stops the *next* snapshot from arriving with the
            // tick missing and un-ticking it on screen.
            push()
        }
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
