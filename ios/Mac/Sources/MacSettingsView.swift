import SwiftUI

/// Where a server is configured, and where it stops being one.
///
/// The app opens without any of this, and that is the point: a shopping list is usable
/// the moment it is installed. Somebody who runs a server comes here to say so, which
/// is a thing a minority of people do once — exactly what a Settings window is for.
///
/// The phones' copy is `SettingsView`; the two say the same things in each platform's
/// own idiom.
struct MacSettingsView: View {
    @Environment(Identity.self) private var identity

    private let cache = Cache.shared

    @State private var server = ServerDirectory.current
    @State private var typed = ""
    @State private var asking = false
    @State private var problem: String?
    @State private var leaving = false
    @State private var joining = false

    var body: some View {
        Form {
            Section {
                LabeledContent("Server", value: server?.origin ?? "None")

                if server == nil {
                    TextField("shopping.example.com", text: $typed)
                        .onSubmit { Task { await use(typed) } }
                    HStack {
                        Button("Use this server") { Task { await use(typed) } }
                            .disabled(asking || typed.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                } else {
                    Button("Stop using this server", role: .destructive) { leaving = true }
                }

                if let problem {
                    Text(problem)
                        .font(.footnote)
                        .foregroundStyle(.red)
                }
            } header: {
                Text("Syncing")
            } footer: {
                Text(
                    server == nil
                        ? "Your lists are on this Mac and nowhere else. Add a server to share them with other devices and other people."
                        : "Your lists are kept on this server and shared with whoever you invite."
                )
                .font(.footnote)
                .foregroundStyle(.secondary)
            }

            // Both need somebody on the other end, so neither exists without one.
            if server != nil {
                Section("Lists") {
                    Button("Join a list…") { joining = true }
                    // Whether this person administers the server is the main window's
                    // to know -- it is the screen that asked -- so the entry point
                    // lives there rather than being decided twice.
                    Button("Sign out") { identity.signOut() }
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .padding(.vertical, 8)
        .sheet(isPresented: $joining) {
            JoinSheet { _ in joining = false }
        }
        // C4. The cache holds rows keyed by ids and uuids that server minted, and
        // history and suggestions belong to an account on it. Keeping them would show
        // one server's lists under no server's name.
        .alert("Stop using this server?", isPresented: $leaving) {
            Button("Cancel", role: .cancel) {}
            Button("Stop", role: .destructive) {
                cache.forgetEverything()
                identity.signOut()
                ServerDirectory.forget()
                server = nil
            }
        } message: {
            Text("The lists on it stay there. This Mac keeps nothing, and starts again on its own.")
        }
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            server = ServerDirectory.current
        }
    }

    private func use(_ entered: String) async {
        switch ServerAddress.parse(entered, allowingCleartext: ServerAddress.allowsCleartext) {
        case .failure(let refusal):
            problem = refusal.localizedDescription
        case .success(let address):
            await adopt(address)
        }
    }

    /// Checked before it is stored, exactly as the phones do it (C2).
    ///
    /// An address is validated by asking it, not by looking at it. Somebody whose
    /// server is not running should be told here rather than by a window full of
    /// nothing afterwards.
    private func adopt(_ address: ServerAddress) async {
        asking = true
        problem = nil
        defer { asking = false }

        switch await ServerDirectory.ask(address) {
        case .success:
            ServerDirectory.remember(address)
            server = address
            typed = ""
        case .failure(let refusal):
            problem = refusal.localizedDescription
        }
    }

}
