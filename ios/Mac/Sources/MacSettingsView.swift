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

    /// Named `current` rather than `server`, which shadowed the global
/// `server(in:)` that reads a host out of a share link.
    @State private var current = ServerDirectory.current
    @State private var typed = ""
    @State private var asking = false
    @State private var problem: String?
    @State private var leaving = false
    @State private var joining = false
    @State private var managingTags = false
    @State private var managingServer = false
    /// Whether this person administers the server, which decides whether the screen
    /// that manages it exists -- and, with a server, whether the categories are theirs
    /// to change. Asked here rather than handed in: this is a `Settings` scene, so it
    /// is a separate window from the one that already knows, with no way to read its
    /// state.
    @State private var isOwner = false
    /// A server named by a share link on the pasteboard. Shown, never adopted -- see
    /// `suggestion` on the phone's `ServerAddressView`.
    @State private var suggestion: ServerAddress?

    /// The same API the main window builds, for the same reason -- see `MacRootView`.
    /// With no server it refuses every connection, and neither screen behind it is
    /// offered in that case.
    private var api: API {
        API(
            baseURL: Config.apiBaseURL,
            token: { await identity.token() },
            remembered: { identity.isRemembered }
        )
    }

    var body: some View {
        Form {
            Section {
                LabeledContent("Server", value: current?.origin ?? "None")

                if current == nil {
                    TextField("shopping.example.com", text: $typed)
                        .onSubmit { Task { await use(typed) } }
                    HStack {
                        Button("Use this server") { Task { await use(typed) } }
                            .disabled(asking || typed.trimmingCharacters(in: .whitespaces).isEmpty)

                        // C7, and it was on the phone only. A share link is the
                        // ordinary way a second person arrives, it carries its own
                        // origin, and a Mac had no way to use that -- somebody sent one
                        // had to pick the host out of the link by hand.
                        //
                        // An explicit click rather than a read on appear: rummaging
                        // through somebody's pasteboard uninvited is not something to
                        // do because a window opened.
                        Button("I have a share link") { readThePasteboard() }
                    }

                    if let suggestion {
                        // Shown, not silently adopted. The host is the thing being
                        // trusted, so it is the thing on screen.
                        Button("Use \(suggestion.origin)") {
                            typed = suggestion.origin
                            Task { await use(suggestion.origin) }
                        }
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
                    current == nil
                        ? "Your lists are on this Mac and nowhere else. Add a server to share them with other devices and other people."
                        : "Your lists are kept on this server and shared with whoever you invite."
                )
                .font(.footnote)
                .foregroundStyle(.secondary)
            }

            // Both need somebody on the other end, so neither exists without one.
            if current != nil {
                Section("Lists") {
                    Button("Join a list…") { joining = true }
                    if isOwner {
                        Button("Who may sign in…") { managingServer = true }
                    }
                    Button("Sign out") { identity.signOut() }
                }
            }

            // Global, so not on a list. Whoever may change them differs: with a server
            // they are the household's vocabulary and the owner's to change; with none
            // there is no household, so anybody using the app may. Absent rather than
            // refusing for somebody who may not -- the service hides the routes from
            // them anyway.
            //
            // The phone has had this since categories became editable. The Mac did
            // not, which meant the one screen for changing them existed on two of the
            // three clients.
            if current == nil || isOwner {
                Section {
                    Button("Categories…") { managingTags = true }
                } footer: {
                    Text("The categories items are grouped under.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .padding(.vertical, 8)
        .sheet(isPresented: $joining) {
            JoinSheet { _ in joining = false }
        }
        // The phone's screens, not copies of them: both bring their own navigation and
        // their own Done, which is why they were written to be presented from here.
        .sheet(isPresented: $managingTags) {
            TagsView(cache: cache, api: api, onDeviceOnly: current == nil)
        }
        .sheet(isPresented: $managingServer) {
            ServerPeopleView(api: api)
        }
        .task { await askWhoIAm() }
        // C4. The cache holds rows keyed by ids and uuids that server minted, and
        // history and suggestions belong to an account on it. Keeping them would show
        // one server's lists under no server's name.
        .alert("Stop using this server?", isPresented: $leaving) {
            Button("Cancel", role: .cancel) {}
            Button("Stop", role: .destructive) {
                cache.forgetEverything()
                identity.signOut()
                ServerDirectory.forget()
                current = nil
            }
        } message: {
            Text("The lists on it stay there. This Mac keeps nothing, and starts again on its own.")
        }
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            current = ServerDirectory.current
            Task { await askWhoIAm() }
        }
    }

    /// Whether this person administers the server.
    ///
    /// Silent on failure and keeps the last answer: a menu item appearing a moment late
    /// is better than a settings window that waits on a question about administration
    /// before it will show somebody how to stop using their server. Nothing behind it
    /// is protected by hiding it -- every route is refused in the service layer to
    /// anybody else.
    private func askWhoIAm() async {
        guard current != nil else {
            isOwner = false
            return
        }
        isOwner = (try? await api.whoAmI().isOwner) ?? isOwner
    }

    /// Takes the origin out of a share link somebody copied.
    ///
    /// The pasteboard is the only route a link has: a share link cannot open this app,
    /// because matching one to an app means an associated domain baked in at build
    /// time and every self-hoster's domain is different.
    private func readThePasteboard() {
        guard let pasted = NSPasteboard.general.string(forType: .string) else {
            problem = "There is no link on the clipboard."
            return
        }

        guard let found = server(in: pasted) else {
            problem = "That does not look like a share link."
            return
        }

        problem = nil
        suggestion = found
    }

    private func use(_ entered: String) async {
        switch ServerAddress.parse(entered) {
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
            current = address
            typed = ""
        case .failure(let refusal):
            problem = refusal.localizedDescription
        }
    }

}
