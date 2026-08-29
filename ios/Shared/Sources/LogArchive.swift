// Nothing here is for the watch. It has no share sheet, no save panel and nowhere to
// put a file somebody could then attach to something -- so it ships its log to the
// phone instead, and the phone's archive carries it. See `WatchDiagnostics`.
#if !os(watchOS)

import Foundation

/// The log, as one file somebody can send.
///
/// A compressed archive rather than the text: what is exported is two or three files
/// (this device's log, the one before it rolled, and the watch's if it has sent one),
/// a share sheet handles one attachment far better than three, and half a megabyte of
/// timestamps compresses to a few tens of kilobytes — which is the difference between
/// something that can be attached to a message and something that cannot.
enum LogArchive {

    /// Builds the archive and says where it is, or nothing if there is no log yet.
    ///
    /// `NSFileCoordinator`'s `.forUploading` is what makes the zip, and it is used
    /// rather than a compression library because it is the same call the system uses
    /// when somebody drags a folder to Mail: no dependency, and the result opens by
    /// double-clicking on every machine anybody would send it to.
    ///
    /// - Note: The caller owns the returned file and should not keep it. It is written
    ///   into the temporary directory, which the system empties on its own schedule —
    ///   deliberately, because this file is a copy of somebody's shopping when tracing
    ///   has been on and it should not outlive the act of sending it.
    static func make() throws -> URL? {
        let manager = FileManager.default
        let folder = LogFile.shared.folder

        // Settled first, so a line written on the way to tapping Export is in it.
        guard !LogFile.shared.settled().isEmpty else { return nil }

        let staged = manager.temporaryDirectory
            .appendingPathComponent("ShoppingList logs", isDirectory: true)
        try? manager.removeItem(at: staged)
        try manager.createDirectory(at: staged, withIntermediateDirectories: true)

        // Everything in the folder rather than this process's two files: the watch's
        // log arrives here under its own name and is exactly what somebody debugging a
        // watch needs, and hard-coding the list is how it would come to be left out the
        // day a second one is added.
        let names = (try? manager.contentsOfDirectory(atPath: folder.path)) ?? []
        for name in names where name.hasSuffix(".log") {
            try? manager.copyItem(
                at: folder.appendingPathComponent(name),
                to: staged.appendingPathComponent(name)
            )
        }

        // What the app was doing, so a log read a week later can be lined up with a
        // build. No server address: it is somebody's hostname and it is not needed to
        // read a log.
        let about = """
            app: \(Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") ?? "?") \
            (\(Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") ?? "?"))
            platform: \(LogFile.thisDevice)
            system: \(ProcessInfo.processInfo.operatingSystemVersionString)
            level: \(LogSettings.level.stored)
            server configured: \(!ServerDirectory.isOnDeviceOnly)
            metrics configured: \(MetricsSettings.endpoint != nil)
            """
        try? Data(about.utf8).write(to: staged.appendingPathComponent("about.txt"))

        var zipped: URL?
        var failure: NSError?
        NSFileCoordinator().coordinate(
            readingItemAt: staged,
            options: .forUploading,
            error: &failure
        ) { readable in
            // Copied out of the coordinator's scratch location before the block returns:
            // what it hands over is deleted the moment this closure ends, and a URL to a
            // file that no longer exists is a share sheet that opens on nothing.
            let destination = manager.temporaryDirectory
                .appendingPathComponent("shopping-list-logs.zip")
            try? manager.removeItem(at: destination)
            try? manager.copyItem(at: readable, to: destination)
            zipped = destination
        }
        if let failure { throw failure }

        try? manager.removeItem(at: staged)
        return zipped
    }
}

#endif
