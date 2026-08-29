import Foundation

/// The one call a composition root makes to start the log and the meter.
///
/// Two things that are configured together and have to be started together, in front of
/// two apps that would otherwise each do it slightly differently -- which is how the
/// Mac came to be missing the categories screen for a year. The watch is not one of the
/// callers: it is told its level by the phone and reports no metrics at all, so it has
/// nothing to start.
enum Diagnostics {

    /// Says the app is up, and starts sending measurements if there is anywhere to send
    /// them.
    ///
    /// Safe to call again, and called again whenever the settings change: choosing a
    /// collector has to start the timer, and giving one up has to stop it.
    static func begin() {
        // The level is read out of storage as `LogBook.shared` is built, so this is the
        // first line the file gets and it is already at the right level.
        Log.info(
            .app, "the app started",
            Detail("standalone", .flag(ServerDirectory.isOnDeviceOnly)),
            Detail("reporting", .flag(MetricsSettings.reporting))
        )
        Metrics.shared.count(
            Measured.launch,
            Tagged("mode", .word("foreground"))
        )
        Metrics.shared.start()

        // Settings changes both of these, and neither is observable storage -- the same
        // reason `ServerDirectory` announces. One observer for the life of the process:
        // `NotificationCenter` keeps it, and there is nothing to remove it from.
        if !started {
            started = true
            NotificationCenter.default.addObserver(
                forName: .diagnosticsChanged,
                object: nil,
                queue: nil
            ) { _ in
                LogBook.shared.level = LogSettings.level
                Metrics.shared.start()
            }
            // Adopting or giving up a server changes whether anything is measured at
            // all, which is the rule in `MetricsSettings.reporting`.
            NotificationCenter.default.addObserver(
                forName: .serverChanged,
                object: nil,
                queue: nil
            ) { _ in
                Metrics.shared.start()
            }
        }
    }

    private nonisolated(unsafe) static var started = false
}
