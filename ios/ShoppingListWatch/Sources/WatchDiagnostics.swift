import Foundation
import WatchConnectivity

/// The watch's half of the diagnostics: it is told how much to write down, and it hands
/// what it wrote to the phone.
///
/// **A watch has no export of its own, and should not.** There is no share sheet on a
/// wrist worth the name, no save panel, and nothing to attach a file to — so a log kept
/// only here is a log nobody can read. It goes to the phone over the link that already
/// exists, and the phone's archive carries it beside its own.
///
/// ## Why it is bounded, three ways
///
/// That link is the thing the watch is for. A snapshot of the lists and a queue of ticks
/// travel on it, both of them time-critical in the one place it matters — standing in a
/// shop — and both of them behind whatever else is queued. `transferFile` is
/// system-scheduled and survives being out of range, which makes it exactly the wrong
/// thing to hand a megabyte to: it will keep trying, in front of the tick somebody just
/// made. So:
///
/// * **A tail, not the file.** ``bytes`` of the most recent lines, which is the end
///   anybody reads anyway.
/// * **One at a time.** Nothing is offered while a transfer is outstanding, so a watch
///   out of range for an afternoon does not build a queue of logs.
/// * **Not more often than ``notMoreOftenThan``.** A log that has just been sent has
///   nothing new worth interrupting the link for.
@MainActor
enum WatchDiagnostics {

    /// The most log worth sending at once.
    ///
    /// Sixty-four kilobytes — around five hundred lines, which covers the last several
    /// minutes at `trace` and the last several days at `warn`. Compressed by the system
    /// on the way across.
    static let bytes = 64 * 1024

    /// The shortest gap between two offers.
    static let notMoreOftenThan: TimeInterval = 5 * 60

    private static var lastOffered: Date?

    /// Adopts the level the phone chose.
    ///
    /// Nothing is stored on the watch. The phone is where the level is set, so the phone
    /// is where it lives -- a watch that remembered its own would go on tracing after
    /// somebody turned tracing off on the device they turned it on from.
    static func adopt(_ context: [String: Any]) {
        guard let said = WatchLink.decode(
            context,
            as: WatchLink.Diagnostics.self,
            under: WatchLink.diagnostics
        ), let level = LogLevel(stored: said.level) else { return }

        guard level != LogBook.shared.level else { return }
        LogBook.shared.level = level
        Log.info(.watch, "the phone set the log level", Detail("level", .word("changed")))

        // Straight away rather than at the next tick. Somebody who has just turned
        // tracing on is somebody sitting in front of both devices trying to reproduce
        // something, and the first thing they will do is press Export.
        offer(force: true)
    }

    /// Hands the phone the tail of this watch's log, if that is worth doing right now.
    ///
    /// - Parameter force: skips the interval, not the other two bounds. For the moment
    ///   the level changes, which is the moment somebody is waiting.
    static func offer(force: Bool = false) {
        let session = WCSession.default
        guard session.activationState == .activated else { return }

        // Nothing on top of what is already going. `transferFile` queues, and a queue of
        // logs is a queue in front of the ticks.
        guard session.outstandingFileTransfers.isEmpty else { return }

        if !force, let last = lastOffered, Date().timeIntervalSince(last) < notMoreOftenThan {
            return
        }

        let tail = LogFile.shared.tail(bytes: bytes)
        guard !tail.isEmpty else { return }

        // Written under the name it should have on the phone: the archive is built from
        // whatever `.log` files are in the folder, and a file called `CoreData-12.tmp`
        // would arrive and be filed as somebody's watch log.
        let staged = FileManager.default.temporaryDirectory
            .appendingPathComponent("watch.log")
        do {
            try tail.write(to: staged, options: .atomic)
        } catch {
            Log.warn(.watch, "could not stage the log", Detail("why", .failure(Plain.Failure(error))))
            return
        }

        lastOffered = Date()
        session.transferFile(staged, metadata: [WatchLink.diagnostics: true])
        Log.info(.watch, "offered the log to the phone", Detail("bytes", .count(tail.count)))
    }
}
