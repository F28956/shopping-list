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

    /// Applies what the watch sent, through the phone's own backend.
    ///
    /// **Through the backend, not through `Cache.shared`.** This read and wrote the old
    /// cache, which stopped being the phone's memory the day the device's own server
    /// took over. On a migrated phone a tick from the wrist was looked up in a store the
    /// phone no longer reads, applied there, and answered `applied` -- so the watch
    /// forgot it, believing it had landed, and it never appeared on the phone. A change
    /// that is acknowledged and then lost is worse than one that fails.
    static func replay(
        _ operations: [SyncOperation],
        through backend: any Backend
    ) async -> [WatchLink.Outcome] {
        var outcomes: [WatchLink.Outcome] = []
        for operation in operations {
            outcomes.append(await apply(operation, through: backend))
        }
        return outcomes
    }

    private static func apply(
        _ operation: SyncOperation,
        through backend: any Backend
    ) async -> WatchLink.Outcome {
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

        // By uuid, because that is the only name the watch has. A list the phone no
        // longer holds is a tick with nowhere to go: `list_gone`, which the drain
        // forgets rather than retries -- the list was deleted while the watch was away,
        // which is an ordinary thing to have happened.
        guard let lists = try? await backend.lists().items,
              let list = lists.first(where: { $0.uuid == operation.list })
        else {
            return WatchLink.Outcome(id: operation.id, outcome: "refused", why: "list_gone")
        }
        guard let rows = try? await backend.items(on: list).items,
              let item = rows.first(where: { $0.uuid == itemUUID })
        else {
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

        // One call, and the backend decides what that means: the device's own server
        // applies it, a `CachingBackend` writes it down and queues it for the server
        // this phone talks to. Either way a tick that arrived from the wrist is this
        // household's change and reaches wherever the phone's own ticks reach.
        //
        // The watch's clock, not this one: the tick may have been made in a shop an
        // hour before the two came back into range, and that is the moment the ordering
        // rules run on -- docs/offline.md.
        do {
            try await backend.setDone(item, on: list, done: done, at: operation.at)
        } catch {
            return WatchLink.Outcome(id: operation.id, outcome: "refused", why: "not_allowed")
        }

        return WatchLink.Outcome(id: operation.id, outcome: "applied", why: nil)
    }
}
