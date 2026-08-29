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

    @State private var exported: Exported?
    @State private var problem: String?

    /// The archive, on its way to a share sheet. `Identifiable` because that is what
    /// `sheet(item:)` wants, and a bare `URL` is not.
    private struct Exported: Identifiable {
        let id = UUID()
        let url: URL
    }

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
        .alert("Record the contents of your lists?", isPresented: warning) {
            Button("Cancel", role: .cancel) { confirming = nil }
            Button("Turn on") {
                if let confirming { apply(confirming) }
                confirming = nil
            }
        } message: {
            Text(
                """
                At this setting the log contains the names of your lists and everything \
                on them, including anything you would not want read out. It is kept on \
                this device until you delete it — but if you send it to somebody, you \
                are sending them your shopping. Turn it back off when you are done.
                """
            )
        }
        .sheet(item: $exported) { archive in
            #if os(iOS)
                ExportSheet(url: archive.url)
            #else
                EmptyView()
            #endif
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
                exported = Exported(url: archive)
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
    /// The system share sheet, holding one file.
    ///
    /// `UIActivityViewController` rather than `ShareLink`, because the archive does not
    /// exist until the button is pressed: a `ShareLink` wants its item up front, which
    /// would mean packing the log every time this screen is drawn.
    struct ExportSheet: UIViewControllerRepresentable {
        let url: URL

        func makeUIViewController(context: Context) -> UIActivityViewController {
            UIActivityViewController(activityItems: [url], applicationActivities: nil)
        }

        func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
    }
#endif
