import SwiftUI

#if os(macOS)
    import AppKit
#else
    import UIKit
#endif

/// The diagnostics, as a person sets them.
///
/// One view for both the phone's Settings screen and the Mac's Preferences window, in
/// the same way `TagsView` and `ServerPeopleView` are one screen presented from two
/// places. The two platforms differ in exactly one thing — where an exported archive
/// goes, a share sheet or a save panel — and that is the only `#if` below.
///
/// It is a `Group` of `Section`s rather than a screen of its own, so each platform's
/// settings keeps its own idiom: a `List` on the phone, a `Form` on the Mac.
struct DiagnosticsSettings: View {
    @Environment(\.capabilities) private var capabilities

    @State private var level = LogSettings.level
    /// A level that would start recording list contents, waiting for somebody to say
    /// they meant it. See ``warning``.
    @State private var confirming: LogLevel?

    @State private var endpoint = MetricsSettings.endpoint?.absoluteString ?? ""
    @State private var headers = MetricsSettings.rawHeaders

    @State private var problem: String?

    /// What the picker offers, which is not every level.
    ///
    /// `error` is missing on purpose: an app that recorded errors and not warnings would
    /// be one that says nothing about a queue that has stopped moving, which is the most
    /// common thing anybody turns this on to look at. "Off" is `warn`, and the footer
    /// says so rather than pretending nothing is written.
    private static let offered: [LogLevel] = [.warn, .info, .debug, .trace]

    private func name(_ level: LogLevel) -> String {
        switch level {
        case .warn: "Off"
        case .info: "Basic"
        case .debug: "Detailed"
        case .trace: "Everything"
        default: level.stored
        }
    }

    var body: some View {
        Group {
            Section {
                Picker("Logging", selection: chosen) {
                    ForEach(Self.offered, id: \.self) { Text(name($0)).tag($0) }
                }

                Button("Export…") { export() }
                    .accessibilityIdentifier("export-log")

                Button("Delete the log", role: .destructive) {
                    LogFile.shared.forget()
                }
                .accessibilityIdentifier("forget-log")

                if let problem {
                    Text(problem).font(.footnote).foregroundStyle(.red)
                }
            } header: {
                Text("Diagnostics")
            } footer: {
                Text(
                    """
                    Off still records failures, which is what makes a report useful. \
                    Detailed and Everything also record what is on your lists — turn \
                    them on only while something is being looked into.
                    """
                )
            }
            // On the section rather than on the `Group` around it: a modifier on a
            // `Group` is applied to each of its children, so up here it was two alerts
            // over one piece of state.
            .alert("Record the contents of your lists?", isPresented: warning) {
                Button("Cancel", role: .cancel) { confirming = nil }
                Button("Turn on") {
                    if let confirming { apply(confirming) }
                    confirming = nil
                }
            } message: {
                Text(
                    """
                    At this setting the log contains the names of your lists and \
                    everything on them, including anything you would not want read out. \
                    It is kept on this device until you delete it — but if you send it \
                    to somebody, you are sending them your shopping. Turn it back off \
                    when you are done.
                    """
                )
            }

            // Only where there is a far end. A device answering for itself has nothing
            // to measure and nobody to tell, and offering the field would be offering to
            // send something about a phone whose owner chose that it would send nothing.
            // The same rule the code follows -- see `MetricsSettings.reporting`.
            if capabilities.syncing {
                Section {
                    TextField("https://collector.example.com/v1/metrics", text: $endpoint)
                        .accessibilityIdentifier("metrics-endpoint")
                        .onSubmit { saveEndpoint() }
                    #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                    #endif

                    TextField(
                        "Authorization: Bearer …",
                        text: $headers,
                        axis: .vertical
                    )
                    .lineLimit(2...5)
                    .accessibilityIdentifier("metrics-headers")
                    #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    #endif

                    Button("Save") { saveEndpoint() }
                } header: {
                    Text("Metrics")
                } footer: {
                    Text(
                        """
                        Sends how long requests take, how deep the queue is and how \
                        often syncing fails, in OpenTelemetry's format, to a collector \
                        you run. One header per line. Nothing about what is on your \
                        lists is included, and nothing is sent at all until you fill \
                        this in.
                        """
                    )
                }
            }
        }
    }

    // MARK: - The level

    private var chosen: Binding<LogLevel> {
        Binding(
            get: { level },
            set: { picked in
                // Shown before it takes effect, not after. A warning that appears once
                // the log is already recording is a warning about something that has
                // already happened.
                if picked.mayCarryContents && !level.mayCarryContents {
                    confirming = picked
                } else {
                    apply(picked)
                }
            }
        )
    }

    private var warning: Binding<Bool> {
        Binding(get: { confirming != nil }, set: { if !$0 { confirming = nil } })
    }

    private func apply(_ picked: LogLevel) {
        level = picked
        LogSettings.level = picked
    }

    // MARK: - Where the collector is

    private func saveEndpoint() {
        let trimmed = endpoint.trimmingCharacters(in: .whitespaces)
        MetricsSettings.rawHeaders = headers

        guard !trimmed.isEmpty else {
            MetricsSettings.endpoint = nil
            problem = nil
            return
        }

        MetricsSettings.endpoint = URL(string: trimmed)
        // Read back rather than trusted: `MetricsSettings` refuses anything that is not
        // an http(s) URL, and a field that silently keeps a rejected address is a field
        // somebody will swear they filled in.
        problem = MetricsSettings.endpoint == nil
            ? "That is not an address a collector could be at."
            : nil
    }

    // MARK: - Getting it off the device

    private func export() {
        problem = nil
        do {
            guard let archive = try LogArchive.make() else {
                problem = "There is nothing logged yet."
                return
            }
            #if os(macOS)
                save(archive)
            #else
                share(archive)
            #endif
        } catch {
            problem = "The log could not be packed up."
            Log.warn(
                .app, "could not build the archive",
                Detail("why", .failure(Plain.Failure(error)))
            )
        }
    }

    #if os(macOS)
        /// A save panel, which is what a Mac does with a file.
        ///
        /// Not a share sheet: the archive is a thing somebody attaches to a mail or drops
        /// in a bug report, and on this platform that starts with it being somewhere in
        /// the Finder.
        private func save(_ archive: URL) {
            let panel = NSSavePanel()
            panel.nameFieldStringValue = "shopping-list-logs.zip"
            panel.canCreateDirectories = true
            guard panel.runModal() == .OK, let destination = panel.url else { return }
            try? FileManager.default.removeItem(at: destination)
            do {
                try FileManager.default.copyItem(at: archive, to: destination)
            } catch {
                problem = "That could not be written."
            }
        }
    #endif
}

#if os(iOS)
    /// Hands one file to the system share sheet.
    ///
    /// Through UIKit and not through `.sheet`, which is not a style choice. Settings is
    /// itself presented as a sheet, and asking SwiftUI to present a second one from a row
    /// inside it *replaces* the first: the visible effect was that Export closed Settings
    /// and offered nothing. The archive was built correctly every time -- 3.7kB of zip,
    /// with the log and an `about.txt` inside it -- and then had nowhere to go.
    ///
    /// `UIActivityViewController` rather than `ShareLink` for the original reason: the
    /// archive does not exist until the button is pressed, and a `ShareLink` wants its
    /// item up front, which would mean packing the log every time this screen is drawn.
    ///
    /// - Note: The popover anchor is for iPad, where a share sheet without one is a
    ///   crash rather than a misplacement.
    @MainActor
    private func share(_ archive: URL) {
        guard let presenter = topmostViewController() else {
            Log.warn(.app, "the archive was built with nowhere to present it")
            return
        }
        let sheet = UIActivityViewController(activityItems: [archive], applicationActivities: nil)
        if let popover = sheet.popoverPresentationController {
            popover.sourceView = presenter.view
            popover.sourceRect = CGRect(
                x: presenter.view.bounds.midX,
                y: presenter.view.bounds.midY,
                width: 0,
                height: 0
            )
            popover.permittedArrowDirections = []
        }
        presenter.present(sheet, animated: true)
    }

    /// The view controller nothing is on top of, which is the one that can present.
    ///
    /// Settings is a sheet on top of the list screen, so the window's root is the wrong
    /// answer -- presenting on it while it already has something presented does nothing
    /// at all, silently.
    @MainActor
    private func topmostViewController() -> UIViewController? {
        let windows = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
        guard var top = windows.first(where: \.isKeyWindow)?.rootViewController else {
            return nil
        }
        while let presented = top.presentedViewController { top = presented }
        return top
    }
#endif
