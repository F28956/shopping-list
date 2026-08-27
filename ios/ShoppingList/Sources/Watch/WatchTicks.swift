import Foundation

/// The phone playing the part of a server, for its watch.
///
/// Only when there is no server — see `WatchLink`. The phone then holds the only copy
/// of the lists there is, so a watch that has been in a shop with nothing in range
/// comes back with a queue, and this is what empties it.
///
/// **Deliberately the same shape as the real thing.** The batch that arrives is the
/// sync route's batch, and what goes back is one outcome per operation in the server's
/// own words, so the watch runs `Outbox.drain` unchanged and the rules about what to
/// forget and what to keep are not written a second time.
///
/// Not a method on a view. A batch arrives whenever WatchConnectivity delivers it,
/// which is routinely while the app is in the background and no list is on screen — so
/// the path from queue to cache cannot run through something that only exists while
/// somebody is looking at it.
@MainActor
enum WatchTicks {

    static func replay(_ operations: [SyncOperation]) async -> [WatchLink.Outcome] {
        var outcomes: [WatchLink.Outcome] = []
        for operation in operations {
            outcomes.append(apply(operation))
        }
        return outcomes
    }

    private static func apply(_ operation: SyncOperation) -> WatchLink.Outcome {
        // Crossing off is the only thing a watch can do, so it is the only thing this
        // accepts. Anything else is refused as invalid rather than kept: a watch that
        // somehow queued it is a watch running a build this one does not understand,
        // and holding it for ever would block everything behind it.
        guard operation.kind == QueuedOperation.Kind.setDone,
              let itemUUID = operation.item,
              let done = operation.done
        else {
            return WatchLink.Outcome(id: operation.id, outcome: "refused", why: "invalid")
        }

        let cache = Cache.shared

        // By uuid, because that is the only name the watch has. A list the phone no
        // longer holds is a tick with nowhere to go: `list_gone`, which the drain
        // forgets rather than retries — the list was deleted while the watch was away,
        // which is an ordinary thing to have happened.
        guard let list = cache.lists().first(where: { $0.uuid == operation.list }) else {
            return WatchLink.Outcome(id: operation.id, outcome: "refused", why: "list_gone")
        }
        guard let item = cache.items(on: list).first(where: { $0.uuid == itemUUID }) else {
            return WatchLink.Outcome(id: operation.id, outcome: "refused", why: "gone")
        }
        guard list.mayEdit else {
            // The one refusal the watch should keep rather than forget: being put back
            // on a list makes this sendable again — see `docs/offline.md` (8).
            return WatchLink.Outcome(id: operation.id, outcome: "refused", why: "not_allowed")
        }

        // Already where the watch wanted it. `already_applied` rather than `applied`,
        // which is the server's own answer to a resend and means the same thing to the
        // drain: landed, forget it.
        guard item.isDone != done else {
            return WatchLink.Outcome(id: operation.id, outcome: "already_applied", why: nil)
        }

        // Queued here as well as applied, because this phone may itself have a server
        // one day: a tick that arrived from the wrist is this household's change and
        // has to reach anywhere the phone's own ticks reach.
        //
        // The watch's clock, not this one. The tick may have been made in a shop an
        // hour before the two came back into range, and it is the moment somebody
        // decided that the ordering rules run on — docs/offline.md.
        cache.outbox.setDone(item, on: list, done: done, at: operation.at)

        let updated = cache.items(on: list).map {
            $0.uuid == itemUUID ? $0.withDone(done) : $0
        }
        cache.remember(items: updated, on: list)

        return WatchLink.Outcome(id: operation.id, outcome: "applied", why: nil)
    }
}
