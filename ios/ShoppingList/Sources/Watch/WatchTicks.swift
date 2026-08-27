import Foundation

/// What the phone does with a crossing-off made on a wrist.
///
/// The watch has no queue, no cache and no server — see `WatchLink`. It says "this was
/// ticked, at this time" and the phone does with that exactly what it does with a tap
/// on its own screen: change the row, queue the operation, and send it if there is
/// anywhere to send it.
///
/// Not a method on a view. A tick arrives whenever WatchConnectivity delivers it, which
/// is routinely while the app is in the background and no list is on screen — so the
/// path from tick to cache cannot run through something that only exists while somebody
/// is looking at it.
@MainActor
enum WatchTicks {
    static func apply(_ tick: WatchLink.Tick) async {
        let cache = Cache.shared

        // By uuid, because that is the only name the watch has. A list the phone no
        // longer holds is a tick with nowhere to go: dropped, and not an error — the
        // list was deleted while the watch was out of range, which is an ordinary
        // thing to have happened.
        guard let list = cache.lists().first(where: { $0.uuid == tick.list }),
              let item = cache.items(on: list).first(where: { $0.uuid == tick.item })
        else { return }

        guard list.mayEdit else { return }
        // Already in the state the watch asked for. Queueing it anyway would be
        // harmless -- setting a row done twice is setting it done -- but it would put
        // a pointless operation in front of everything behind it in the queue.
        guard item.isDone != tick.done else { return }

        // The watch's clock, not this one. The tick may have been made in a shop an
        // hour before the phone came back into range, and it is the moment somebody
        // decided that the ordering rules run on -- docs/offline.md.
        cache.outbox.setDone(item, on: list, done: tick.done, at: tick.at)

        let updated = cache.items(on: list).map {
            $0.uuid == tick.item ? $0.withDone(tick.done) : $0
        }
        cache.remember(items: updated, on: list)
    }
}
