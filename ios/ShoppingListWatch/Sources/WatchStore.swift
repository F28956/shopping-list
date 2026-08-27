import Foundation
import Observation
import WatchConnectivity

/// What this watch knows, and the only thing that knows it.
///
/// The phone is the server — see `WatchLink`. This holds the last picture the phone
/// sent and the ticks made here that have not reached it yet, and both of those are
/// **stored by the system rather than by this app**:
///
/// * `WCSession.receivedApplicationContext` is the last snapshot, kept across launches.
///   So there is no database on this watch, and there does not need to be.
/// * `WCSession.outstandingUserInfoTransfers` is the queue of ticks the system is still
///   trying to deliver, also kept across launches. So there is no outbox either.
///
/// That is the whole reason this app got smaller rather than larger: the two things it
/// used to need persistence for are things WatchConnectivity already persists.
@MainActor
@Observable
final class WatchStore: NSObject, WCSessionDelegate {

    /// The lists, as the phone last described them, with this watch's unsent ticks laid
    /// back over the top — the same trick the phone plays on the server's answer. Without
    /// it, a tick would show for a moment and then be undone by the next snapshot, which
    /// was sent before the phone had heard about it.
    private(set) var lists: [WatchLink.ListOnTheWatch] = []

    /// Whether the phone has ever said anything.
    ///
    /// What earns an empty state. A watch that has heard nothing is not a watch whose
    /// owner has no lists, and saying "no lists" to somebody who has ten is worse than
    /// saying nothing.
    private(set) var heard = false

    /// There is no server anywhere in this arrangement, which the status dot says
    /// differently from "waiting".
    private(set) var onDeviceOnly = false

    /// Ticks the system has not yet handed to the phone.
    private(set) var waiting = 0

    /// This watch's own ticks, by item uuid, until the phone confirms them by sending
    /// a snapshot that already agrees.
    private var pending: [String: Bool] = [:]

    override init() {
        super.init()
        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()
        // Whatever arrived while this app was not running. Read rather than waited for:
        // the system does not re-deliver a context on launch, it just has it.
        adopt(WCSession.default.receivedApplicationContext)
    }

    // MARK: - Crossing things off

    /// Crosses something off, or puts it back.
    ///
    /// The row changes here and now. Whether the phone is in range does not come into
    /// it: a tick in a shop is a decision somebody has already made, and an app that
    /// waits for a phone before showing it has made them wait for something they cannot
    /// influence. `transferUserInfo` keeps it and retries until the phone takes it.
    func toggle(_ item: WatchLink.ItemOnTheWatch, on list: WatchLink.ListOnTheWatch) {
        let done = !item.done
        pending[item.id] = done
        restack()

        send(WatchLink.Tick(list: list.id, item: item.id, done: done, at: Date()))
    }

    /// Gets one tick to the phone, by whichever route can carry it.
    ///
    /// Two routes, and both are needed:
    ///
    /// * **`sendMessage`** when the phone is reachable — awake, nearby, listening. It
    ///   arrives in milliseconds, which is what somebody standing next to their phone
    ///   expects, and it tells us if it failed.
    /// * **`transferUserInfo`** otherwise, and as the fallback when a send fails. The
    ///   system keeps it, retries it, and delivers it in order whenever the phone comes
    ///   back — which is the case this app exists for: a shop, and a phone in a pocket
    ///   that has gone to sleep.
    ///
    /// Sending only the queued way looked right and was not: a tick made with the phone
    /// in your hand sat there, because "eventually" is a promise about the worst case
    /// and this was the best one.
    private func send(_ tick: WatchLink.Tick) {
        let session = WCSession.default
        guard session.activationState == .activated else {
            queue(tick)
            return
        }

        guard session.isReachable else {
            queue(tick)
            return
        }

        session.sendMessage(
            WatchLink.encode(tick),
            replyHandler: nil,
            errorHandler: { [weak self] _ in
                // Reachable a moment ago and not any more, which is ordinary: a wrist
                // drops out of range mid-gesture. The queued route still has it.
                Task { @MainActor in self?.queue(tick) }
            }
        )
    }

    private func queue(_ tick: WatchLink.Tick) {
        WCSession.default.transferUserInfo(WatchLink.encode(tick))
        countWaiting()
    }

    // MARK: - What arrives

    private func adopt(_ context: [String: Any]) {
        guard let snapshot = WatchLink.decode(context, as: WatchLink.Snapshot.self) else {
            return
        }
        latest = snapshot
        onDeviceOnly = snapshot.onDeviceOnly
        heard = true

        // A tick the phone has caught up with stops being this watch's business. Kept
        // by value rather than by counting transfers: the system's queue empties when
        // it *delivers*, which is before the phone has necessarily written anything, so
        // agreement is the honest signal that a tick has landed.
        for list in snapshot.lists {
            for item in list.items where pending[item.id] == item.done {
                pending.removeValue(forKey: item.id)
            }
        }
        restack()
        countWaiting()
    }

    private var latest = WatchLink.Snapshot()

    /// Lays this watch's unsent ticks over the phone's picture.
    private func restack() {
        lists = latest.lists.map { list in
            var list = list
            list.items = list.items.map { item in
                guard let ticked = pending[item.id] else { return item }
                var item = item
                item.done = ticked
                return item
            }
            return list
        }
    }

    private func countWaiting() {
        waiting = WCSession.default.outstandingUserInfoTransfers.count
    }

    // MARK: - Session

    nonisolated func session(
        _ session: WCSession,
        activationDidCompleteWith state: WCSessionActivationState,
        error: Error?
    ) {
        Task { @MainActor in
            adopt(WCSession.default.receivedApplicationContext)
        }
    }

    nonisolated func session(
        _ session: WCSession,
        didReceiveApplicationContext applicationContext: [String: Any]
    ) {
        Task { @MainActor in adopt(applicationContext) }
    }

    nonisolated func session(
        _ session: WCSession,
        didFinish userInfoTransfer: WCSessionUserInfoTransfer,
        error: Error?
    ) {
        Task { @MainActor in countWaiting() }
    }
}
