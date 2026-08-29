import Foundation

/// The rolling copy of the log, so there is something to export.
///
/// Unified logging is where these lines belong and it is not enough on its own: what
/// `Logger` keeps is decided by the system, is trimmed under memory pressure, and can
/// only be read off the device by somebody with a cable and Console. A person in a shop
/// whose watch stopped syncing has none of that. So every line is also appended here,
/// and this is what the share sheet hands over.
///
/// ## Why it is capped, and capped this way
///
/// Two files of a fixed size and no more. A log that grows until somebody notices is a
/// log that fills a phone, and the failures worth reading are the recent ones — a queue
/// that has not drained for an hour has said so a thousand times and the first nine
/// hundred are identical.
///
/// Rolling rather than trimming: trimming means rewriting the file on every append,
/// which is the sort of thing that looks free until it is doing it forty times a second
/// behind a list that is scrolling. Two files means one rename when the first fills,
/// and the export sends both.
final class LogFile: @unchecked Sendable {

    /// This process's file. The watch has one of these too, in its own container, and
    /// ships it to the phone — see `WatchDiagnostics`.
    static let shared = LogFile(in: LogFile.defaultFolder)

    /// Where a device keeps its own log.
    ///
    /// Beside the databases rather than in Caches: the system empties Caches whenever
    /// it likes, and a diagnostic that is gone by the time somebody goes to export it
    /// is worse than none — they will report that the button does nothing.
    static var defaultFolder: URL {
        let support = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("ShoppingList", isDirectory: true)
        return support.appendingPathComponent("logs", isDirectory: true)
    }

    /// How big one file gets before it becomes the older one.
    ///
    /// A quarter of a megabyte, twice, so the whole of what is kept is half a megabyte
    /// of text — around four thousand lines, which is a long afternoon at `info` and
    /// about twenty minutes at `trace`. It compresses to a few tens of kilobytes, which
    /// is what matters: the export is something somebody attaches to a message.
    static let sizeLimit = 256 * 1024

    let folder: URL
    /// What this process wrote. Named for the device it came from, because the phone's
    /// export carries the watch's file beside its own.
    let current: URL
    let previous: URL

    private let limit: Int
    private let queue: DispatchQueue
    private var handle: FileHandle?
    private var written: Int = 0

    private static let stamp: DateFormatter = {
        let formatter = DateFormatter()
        // Fixed locale and fixed zone: a log read on a different continent from the one
        // it was written on has to be comparable with the server's, and a device set to
        // a Buddhist calendar should not produce timestamps nobody can line up.
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'"
        return formatter
    }()

    init(in folder: URL, named name: String = LogFile.thisDevice, limit: Int = LogFile.sizeLimit) {
        self.folder = folder
        self.current = folder.appendingPathComponent("\(name).log")
        self.previous = folder.appendingPathComponent("\(name)-previous.log")
        self.limit = limit
        self.queue = DispatchQueue(label: "shoppinglist.log.\(name)", qos: .utility)
    }

    /// What this device's file is called inside the archive.
    static var thisDevice: String {
        #if os(watchOS)
            return "watch"
        #elseif os(macOS)
            return "mac"
        #else
            return "phone"
        #endif
    }

    /// Adds a line, later.
    ///
    /// Asynchronous on purpose. Every call site is a backend, a queue or a session
    /// delegate in the middle of doing something else, and a synchronous write would put
    /// a disk on the path of a tick being crossed off. Ordering is kept because the
    /// queue is serial.
    func append(_ level: LogLevel, _ area: LogArea, _ line: String) {
        let stamped = "\(Self.stamp.string(from: Date())) \(level.initial) [\(area.rawValue)] \(line)\n"
        queue.async { [weak self] in self?.put(stamped) }
    }

    private func put(_ text: String) {
        let bytes = Data(text.utf8)

        if handle == nil { open() }
        guard let handle else { return }

        // Rolled *before* the write rather than after, so a file never exceeds the cap
        // even by one line. After the fact is how a `trace` line carrying a whole list
        // could leave a file half again as big as the limit says.
        if written + bytes.count > limit {
            roll()
            guard let rolled = self.handle else { return }
            try? rolled.write(contentsOf: bytes)
            written = bytes.count
            return
        }

        try? handle.write(contentsOf: bytes)
        written += bytes.count
    }

    private func open() {
        let manager = FileManager.default
        try? manager.createDirectory(at: folder, withIntermediateDirectories: true)
        if !manager.fileExists(atPath: current.path) {
            manager.createFile(atPath: current.path, contents: nil)
        }
        handle = try? FileHandle(forWritingTo: current)
        // Appending to what is already there. A relaunch that truncated the file would
        // throw away the lines written just before a crash, which are the ones anybody
        // is looking for.
        written = Int((try? handle?.seekToEnd()) ?? 0)
    }

    private func roll() {
        try? handle?.close()
        handle = nil
        let manager = FileManager.default
        try? manager.removeItem(at: previous)
        try? manager.moveItem(at: current, to: previous)
        open()
    }

    /// Everything kept, oldest first, or nothing if this device has never logged.
    ///
    /// Waits for anything queued, so a line written on the way to tapping Export is in
    /// what is exported.
    func settled() -> [URL] {
        queue.sync {
            try? handle?.synchronize()
        }
        return [previous, current].filter { FileManager.default.fileExists(atPath: $0.path) }
    }

    /// Throws away what is kept.
    ///
    /// Offered beside the level in Settings: somebody who has just turned tracing off is
    /// somebody holding a file full of their shopping, and the way to not hold it is a
    /// button rather than a reinstall.
    func forget() {
        queue.sync {
            try? handle?.close()
            handle = nil
            written = 0
            try? FileManager.default.removeItem(at: current)
            try? FileManager.default.removeItem(at: previous)
        }
    }

    /// The tail of what is kept, at most `bytes` of it.
    ///
    /// What the watch ships to the phone. The tail rather than the head because the
    /// interesting end of a log is the end, and capped because the link between the two
    /// is shared with the thing the watch is for — see `WatchDiagnostics`.
    func tail(bytes: Int) -> Data {
        queue.sync { try? handle?.synchronize() }

        var collected = Data()
        for url in settled() {
            guard let part = try? Data(contentsOf: url) else { continue }
            collected.append(part)
        }
        guard collected.count > bytes else { return collected }

        // Cut at a line boundary, so what arrives does not start halfway through a
        // timestamp and read as a corrupt file.
        let tail = collected.suffix(bytes)
        guard let newline = tail.firstIndex(of: 0x0A) else { return tail }
        return Data(tail[tail.index(after: newline)...])
    }
}
