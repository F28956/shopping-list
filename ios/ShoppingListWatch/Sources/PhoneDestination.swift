import Foundation
import WatchConnectivity
import os

/// The phone, as somewhere this watch's queue can be emptied to.
///
/// Only used when there is no server — see `WatchLink`. The phone then holds the only
/// copy of the lists there is, so it is the far end, and it answers a batch of queued
/// operations exactly as the sync route would: one outcome per operation, in the
/// server's own vocabulary. That is what lets `Outbox.drain` run unchanged in both
/// modes rather than being written twice.
struct PhoneDestination: Destination {

    /// The phone is asleep, out of range, or has no app running.
    ///
    /// Thrown rather than swallowed, because the drain has a correct answer for it:
    /// nothing is forgotten, nothing is said, and everything is still queued for the
    /// next time. Exactly what it does when a server cannot be reached.
    struct OutOfReach: Error {}

    func sync(_ operations: [SyncOperation]) async throws -> [AppliedOperation] {
        let session = WCSession.default
        guard session.activationState == .activated else { throw OutOfReach() }

        guard session.isReachable else {
            // Not in range, so hand it to the system instead of only keeping it.
            //
            // `transferUserInfo` is queued by watchOS and delivered whenever the two
            // devices next find each other -- with **neither app running**, waking the
            // phone in the background to take it. Without this a tick made in a shop
            // waits for somebody to open the watch app again while the phone is near,
            // because that is the only thing that drains the queue.
            //
            // The operations stay queued here all the same. There is no reply to a
            // transfer, so nothing can be forgotten on the strength of it; the phone
            // answers `already_applied` when the watch next reaches it properly, which
            // is what clears them. Applying a crossing-off twice is the same as
            // applying it once, which is what makes sending it twice safe.
            session.transferUserInfo(WatchLink.encode(WatchLink.SyncRequest(operations: operations)))
            throw OutOfReach()
        }

        let request = WatchLink.SyncRequest(operations: operations)
        let reply: WatchLink.SyncReply = try await withCheckedThrowingContinuation { continuation in
            // `resume` exactly once: WatchConnectivity calls one handler or the other,
            // but a continuation resumed twice is a crash rather than a bug report.
            let answered = OSAllocatedUnfairLock(initialState: false)
            func finish(_ result: Result<WatchLink.SyncReply, Error>) {
                let first = answered.withLock { done -> Bool in
                    defer { done = true }
                    return !done
                }
                if first { continuation.resume(with: result) }
            }

            session.sendMessage(
                WatchLink.encode(request),
                replyHandler: { answer in
                    guard let reply = WatchLink.decode(answer, as: WatchLink.SyncReply.self) else {
                        // A phone running a build that does not know this message. Not
                        // an empty answer, which the drain would read as "none of it
                        // applied" and forget the lot.
                        finish(.failure(OutOfReach()))
                        return
                    }
                    finish(.success(reply))
                },
                errorHandler: { _ in finish(.failure(OutOfReach())) }
            )
        }

        // Into the shape the drain reads. `item` and `list` are always nil: the watch
        // only ever queues a crossing-off, which creates nothing.
        return reply.outcomes.map {
            AppliedOperation(id: $0.id, outcome: $0.outcome, item: nil, list: nil, why: $0.why)
        }
    }
}
