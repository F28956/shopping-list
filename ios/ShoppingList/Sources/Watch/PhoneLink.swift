import Foundation
import UIKit
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

    /// Where the phone reads its own lists.
    ///
    /// Set by the app as it starts, like `token`, because a backend needs an identity
    /// and this is built before one exists. Nil until then, and a nil backend sends
    /// nothing rather than sending an empty list -- a watch told "you have no lists" is
    /// worse than a watch told nothing.
    var backend: (@Sendable () async -> (any Backend)?)?
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

        // Going away is the last chance to say anything.
        //
        // Every other push is debounced by 300ms, because loading a list writes the
        // lists and then the items and a screenful of ticks writes once each -- sending
        // a snapshot per write would spend the link on states nobody will ever see. But
        // a debounce is a Task that is sleeping, and an app that is closed while it
        // sleeps never wakes up: the last thing somebody did before putting the phone
        // in their pocket was exactly the thing the watch never heard about.
        NotificationCenter.default.addObserver(
            forName: UIApplication.willResignActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.pushNow() }
        }
    }

    /// Pushes without waiting for the burst to settle.
    ///
    /// For the one case where there may not be a later: the app is going away.
    private func pushNow() {
        pending?.cancel()
        push()
    }

    /// Pushes once, after the current burst of changes.
    ///
    /// Loading a list writes the lists and then the items, and a screenful of ticks
    /// writes once each. Sending a snapshot per write would spend the link on states
    /// nobody will ever see; the watch only wants the one at the end.
    private func pushSoon() {
        // A nudge raised by this type's own reading is not news.
        //
        // `snapshot()` reads through the backend, and a `CachingBackend` read writes
        // what it found to the cache -- which is a change, which is a nudge, which asks
        // for another snapshot. Left alone that is a loop at the speed of the network:
        // it ran at twelve hundred requests a minute against a server with nothing to
        // say, and every one of them was this type talking to itself.
        //
        // Remembered rather than dropped. A change made somewhere else while a snapshot
        // is being built is real news, and it arrives in exactly this window.
        guard !reading else {
            heardWhileReading = true
            return
        }

        pending?.cancel()
        pending = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            push()
        }
    }

    private var pending: Task<Void, Never>?

    /// Whether a snapshot is being read right now. See ``pushSoon()``.
    private var reading = false

    /// Whether anything asked for a push while one was being read.
    private var heardWhileReading = false

    /// Watches everything this phone holds, so that a change made here reaches the wrist.
    private var following: Task<Void, Never>?

    /// Hands over the backend this phone reads, and starts watching it.
    ///
    /// One call rather than two, because the two must not drift apart: a `backend` set
    /// without a `follow` is a watch that receives one snapshot at launch and nothing
    /// afterwards, which is exactly the state this app shipped in.
    func use(_ backend: any Backend) {
        self.backend = { backend }
        follow()
        push()
    }

    /// Pushes whenever anything this phone holds moves.
    ///
    /// `.cacheChanged` was the only trigger, and it stopped firing the day the device's
    /// own server took over: `LocalBackend` writes `device.sqlite` through `domain` and
    /// never touches the cache. So an item crossed off on the phone reached the wrist
    /// only if something else happened to poke the link -- in practice never. The watch
    /// showed a picture that was right when it arrived and then went quietly stale,
    /// which looks identical to a picture that is correct.
    ///
    /// Both channels, because `domain` keeps them apart deliberately: a list's rows and
    /// the set of lists are announced separately so that a screen watching one list does
    /// not re-read when another moves. "Anything at all" is the union of the two, and it
    /// has to be rebuilt whenever the set changes -- a list made a moment ago has no row
    /// stream yet.
    /// The lists ``follow()`` is subscribed to.
    ///
    /// Compared against what a snapshot actually found, which is a read this was doing
    /// anyway. The alternative -- asking the backend for the lists inside the nudge
    /// handler -- is a network round trip per nudge, and the read writes the cache,
    /// which raises another nudge. That version ran at seventy requests a second.
    private var subscribedTo: Set<String> = []

    private func follow() {
        following?.cancel()
        following = Task { @MainActor [weak self] in
            guard let self, let backend = await self.backend?() else { return }
            let lists = (try? await backend.lists().items) ?? []
            self.subscribedTo = Set(lists.map(\.uuid))

            // One task per list plus one for the set, and every one of them does the
            // same thing: ask for a push. `pushSoon` is debounced, so a burst of nudges
            // costs one snapshot rather than one each.
            //
            // Nothing here decides to rebuild. Two earlier versions did -- one ending
            // the round whenever any stream returned, one asking whether the set had
            // changed on every nudge -- and both turned into a loop, because a read
            // writes the cache and a cache write is another nudge. What the
            // subscriptions are for is *sending*; noticing that the set of lists moved
            // is `hand(over:)`'s job, from the snapshot it was building anyway.
            await withTaskGroup(of: Void.self) { group in
                for list in lists {
                    group.addTask { @MainActor in
                        guard let stream = try? await backend.changes(on: list) else { return }
                        do {
                            for try await _ in stream { self.pushSoon() }
                        } catch {}
                    }
                }
                group.addTask { @MainActor in
                    guard let stream = try? await backend.listChanges() else { return }
                    do {
                        for try await _ in stream { self.pushSoon() }
                    } catch {}
                }
            }
        }
    }

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

        Task { await hand(over: session) }
    }

    /// Builds the picture and hands it over.
    ///
    /// Its own step because reading the backend is asynchronous now -- it may be a
    /// database on this device or a server -- and `push` is called from notification
    /// handlers that are not.
    private func hand(over session: WCSession) async {
        reading = true
        let taken = await snapshot()

        // Whether the read found anything the watch has not already been told.
        //
        // This is what tells our own nudges from real ones. Every read raises nudges,
        // because a `CachingBackend` read writes what it found -- so "something asked
        // for a push while I was reading" is true on every single pass and cannot mean
        // anything on its own. What can: if the snapshot is identical to the last one
        // sent, nothing happened, and whatever nudged was this type talking to itself.
        //
        // Answering that with another read is the loop. It ran at twelve hundred
        // requests a minute, then at five hundred when the nudge was merely delayed
        // rather than judged.
        let changed = taken != nil && taken != lastSent

        // A moment after the read, because the cache's own observation fires just behind
        // the write that caused it -- ending the window on the last line would let the
        // tail of our own writes back in.
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(500))
            reading = false
            let heard = heardWhileReading
            heardWhileReading = false
            // Only when the last look actually found something. A change made elsewhere
            // during the window is then followed up; our own echo is not.
            if heard && changed { pushSoon() }
        }

        guard let snapshot = taken, changed else { return }

        // The set of lists moved, so the subscriptions are watching the wrong ones.
        // Noticed here because the snapshot has just read every list -- no extra round
        // trip, and only when it genuinely differs.
        let found = Set(snapshot.lists.map(\.id))
        if found != subscribedTo { follow() }

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
    /// Read from wherever the phone reads, deliberately -- so the watch agrees with the
    /// phone even when both are out of signal, and on a device with no server what is
    /// sent is not a copy of anything, it is the lists.
    ///
    /// **Which store that is stopped being obvious.** This read `Cache.shared`, which
    /// was the phone's memory until the device's own server took over; after a
    /// migration the phone writes `device.sqlite` and nothing writes the old cache
    /// again. The watch would have gone on receiving the snapshot taken the moment the
    /// migration ran, for ever -- every list made and every item ticked off invisible to
    /// it, with no error anywhere, because a frozen picture looks exactly like a
    /// picture that has not changed.
    ///
    /// So it reads the *backend*, which is whatever the phone's own screens read.
    private func snapshot() async -> WatchLink.Snapshot? {
        var remaining = WatchLink.cap
        // Nothing to send before the app has built one. The watch keeps the last
        // picture it had, which is the right answer for "I do not know yet".
        guard let backend = await backend?() else { return nil }

        let units = (try? await backend.units()) ?? []
        let visible = (try? await backend.lists())?.items ?? []

        var lists: [WatchLink.ListOnTheWatch] = []
        for list in visible {
            let tags = (try? await backend.tags(orderedFor: list)) ?? []
            let all = (try? await backend.items(on: list))?.items ?? []

            // In the order the shop is walked, before the cap, so what is dropped is
            // the tail of the walk rather than an arbitrary slice of it. Crossed-off
            // rows come last: they are the least useful thing to spend the budget on.
            let outstanding = grouped(all.filter { !$0.isDone }, by: tags).flatMap(\.items)
            let done = all.filter(\.isDone)
            let ordered = outstanding + done

            let sent = ordered.prefix(remaining)
            remaining -= sent.count

            lists.append(WatchLink.ListOnTheWatch(
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
            ))
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
            guard let backend = await PhoneLink.shared.backend?() else {
                // Nothing to apply them to yet. Refusing nothing and answering nothing
                // leaves them queued on the watch, which is where they should stay.
                replyHandler([:])
                return
            }
            let outcomes = await WatchTicks.replay(request.operations, through: backend)
            replyHandler(WatchLink.encode(WatchLink.SyncReply(outcomes: outcomes)))
            // The watch's picture is now out of date by exactly the changes it just
            // sent. It has already drawn them, so this is not what makes it look
            // right — it is what stops the next snapshot from arriving without them
            // and undoing them on screen.
            push()
        }
    }

    /// A batch that arrived out of range, delivered by the system later.
    ///
    /// The other half of `PhoneDestination`'s fallback: no reply is possible, so the
    /// watch has not forgotten these and will offer them again when it can talk
    /// properly. Applying them now is what makes the phone right in the meantime, and
    /// applying them twice is the same as applying them once.
    nonisolated func session(
        _ session: WCSession,
        didReceiveUserInfo userInfo: [String: Any]
    ) {
        guard let request = WatchLink.decode(userInfo, as: WatchLink.SyncRequest.self) else {
            return
        }

        Task { @MainActor in
            guard let backend = await PhoneLink.shared.backend?() else { return }
            _ = await WatchTicks.replay(request.operations, through: backend)
            // The watch's picture is now out of date by exactly what it just sent.
            PhoneLink.shared.push()
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
