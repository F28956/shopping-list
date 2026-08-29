import AuthenticationServices
import SwiftUI

@main
struct ShoppingListMacApp: App {
    @State private var identity = Identity()

    init() {
        // Before anything else this app does, so that whatever goes wrong at startup has
        // somewhere to be written down. Nothing below `warn` is written until somebody
        // turns logging on in Settings -- see `LogBook.level`.
        Diagnostics.begin()

    }

    var body: some Scene {
        WindowGroup {
            MacRootView()
                .environment(identity)
                .frame(minWidth: 620, minHeight: 420)
                // Only when there is somewhere to sign in to. On a Mac kept to itself
                // there is nobody to restore and nothing that would use the answer.
                .task {
                    if !ServerDirectory.isOnDeviceOnly { await identity.restore() }
                }
        }
        // A list is a document-shaped thing: one window, resizable, remembered.
        .defaultSize(width: 820, height: 560)
        .commands {
            // Nothing under it yet, but replacing New Window's default here stops
            // the menu offering a second window that would fight the first for the
            // same list selection.
            CommandGroup(replacing: .newItem) {}
        }

        // Where a server is configured, under ⌘, like everything else on this
        // platform. The phones put it behind a gear for the same reason it is here:
        // a shopping list is usable the moment it is installed, and hosting is
        // something a minority of people set up once.
        Settings {
            MacSettingsView()
                .environment(identity)
        }
    }
}

struct MacRootView: View {
    @Environment(Identity.self) private var identity

    /// Re-read when settings change the answer, because `ServerDirectory` is storage
    /// rather than observable state and nothing would otherwise tell SwiftUI.
    @State private var hasServer = !ServerDirectory.isOnDeviceOnly

    /// What this app can offer, supplied once for every screen below.
    ///
    /// Held as state rather than read where it is used, so that choosing a server in
    /// Settings changes what is offered without relaunching -- the same reason
    /// `hasServer` is state. Every screen reads it from the environment; none of them
    /// reads `ServerDirectory`.
    @State private var capabilities = Capabilities.current

    /// Opened once and kept, rather than opened again every time this body runs.
    ///
    /// `shopping` is a computed property, so `LocalBackend.readyForUse()` ran on every
    /// re-render: a fresh SQLite connection and a fresh set of watcher threads each
    /// time, none of them closed. The phone had the same fault and the same fix.
    @State private var api: API?
    @State private var standalone: LocalBackend?

    private func open() {
        let api = API(
            // With no server this is a placeholder that refuses every connection,
            // which is the point: the failure is a transport failure, and the app
            // already knows how to queue through one of those.
            baseURL: Config.apiBaseURL,
            token: { await identity.token() },
            remembered: { identity.isRemembered }
        )
        self.api = api
        // As on the phone: the Mac answers for itself when nobody has chosen a
        // server, and falls back to the old path if the database will not open. And
        // when a server is chosen, what this Mac holds is handed to the cache first, or
        // adopting one would show an empty account with everything still on disk.
        if ServerDirectory.isOnDeviceOnly {
            LocalBackend.mayHandOverAgain()
            self.standalone = LocalBackend.readyForUse()
        } else {
            LocalBackend.handOverToAServer()
            self.standalone = nil
        }
    }

    var body: some View {
        Group {
            if hasServer {
                signedInOrNot
            } else {
                // The default, and it opens straight into the lists. A shopping list
                // should be usable the moment it is installed, not open by asking a
                // question about hosting -- so there is no first-run screen, nothing
                // to dismiss, and nothing to sign in to. Somebody who runs a server
                // goes and says so in Settings.
                shopping
            }
        }
        .environment(\.capabilities, capabilities)
        .task { open() }
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            hasServer = !ServerDirectory.isOnDeviceOnly
            capabilities = .current
            // A different mode is a different backend -- this Mac's own server, or a
            // cache in front of somebody else's.
            open()
            // Adopting a server means there is now somebody to be signed in as, and
            // until something asks, `identity.state` stays `.unknown` -- which
            // `signedInOrNot` renders as a spinner. It was asked only in the launch
            // task, and only when a server was already configured, so a device that
            // started standalone and then chose one sat on that spinner for ever with
            // no way out but relaunching.
            if hasServer { Task { await identity.restore() } }
        }
    }

    @ViewBuilder
    private var signedInOrNot: some View {
        switch identity.state {
        case .unknown:
            ProgressView()
        case .signedOut:
            MacSignInView()
        case .signedIn:
            shopping
        }
    }

    /// The lists.
    ///
    /// The same view either way. With no server every request it makes fails and
    /// everything queues, which is exactly what it already does with no signal --
    /// "no server" and "no connection" are the same state, and the app only ever knew
    /// how to be in one of them.
    @ViewBuilder
    private var shopping: some View {
        if let api {
            MacShoppingView(api: api, standalone: standalone)
        } else {
            ProgressView()
        }
    }
}

/// Signing in to the server this Mac has been pointed at.
///
/// Reached only once somebody has configured a server, so it is never what a fresh
/// install opens on.
///
/// It leads back out as well as in. Preferences is a `Settings` scene and so is
/// reachable under ⌘, even from here, which made this a softer trap than the phones'
/// -- but only for somebody who thinks to try it. Somebody who typed the wrong address
/// is looking at a sign-in button and a server they cannot reach, and the way out
/// should be on the screen that is the problem.
struct MacSignInView: View {
    @Environment(Identity.self) private var identity
    @Environment(\.colorScheme) private var scheme

    private let cache = Cache.shared

    /// Which server this is. "Use a different server" and "use this Mac only" are not
    /// decisions anybody can take without knowing what they are leaving.
    private let server = ServerDirectory.current

    @State private var leaving = false

    var body: some View {
        VStack(spacing: 16) {
            Text("Shopping list")
                .font(.largeTitle.weight(.semibold))
            Text("The same lists as the phone, with a keyboard.")
                .foregroundStyle(.secondary)

            if let server {
                LabeledContent("Server", value: server.origin)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: 280)
            }

            // Apple's own button rather than one styled to look like it: the mark,
            // the wording and the corner radius are the part people recognise before
            // they read anything, and it is what the guidelines ask for besides.
            SignInWithAppleButton(.signIn, onRequest: identity.request) { result in
                Task { await identity.adopt(result) }
            }
            // There is no `.automatic`: the three styles are black, white and
            // white-outlined, and picking one is the caller's job. A Mac window
            // follows the system appearance, so the mark has to as well or it is a
            // black slab in a dark window.
            .signInWithAppleButtonStyle(scheme == .dark ? .white : .black)
            .frame(width: 240, height: 40)
            .accessibilityIdentifier("sign-in")

            if let error = identity.lastError {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }

            // The two mistakes somebody makes here: the address was wrong, or a server
            // was never wanted in the first place. Changing it is Preferences' job and
            // this points at it rather than growing a second copy of that form.
            HStack(spacing: 16) {
                Button("Change server…") {
                    // The same thing ⌘, does. Named here because somebody stuck on
                    // this screen is not thinking about the menu bar.
                    if #available(macOS 14, *) {
                        NSApp.sendAction(
                            Selector(("showSettingsWindow:")), to: nil, from: nil
                        )
                    } else {
                        NSApp.sendAction(
                            Selector(("showPreferencesWindow:")), to: nil, from: nil
                        )
                    }
                }
                Button("Use this Mac only") { leaving = true }
            }
            .font(.footnote)
            .buttonStyle(.link)
        }
        .padding(40)
        // C4. The lists on screen after this belong to no server, and rows keyed by
        // ids that one minted would be showing its lists under nobody's name. Said out
        // loud rather than done quietly, in the same words Preferences uses, because
        // it is the same act.
        .alert("Use this Mac only?", isPresented: $leaving) {
            Button("Cancel", role: .cancel) {}
            Button("Use this Mac only", role: .destructive) {
                cache.forgetEverything()
                identity.signOut()
                ServerDirectory.onlyThisDevice()
            }
        } message: {
            Text(
                """
                Your lists will stay on this Mac and nothing will be synced. This \
                removes everything stored here, including anything still waiting to \
                be sent. You can add a server again in Preferences.
                """
            )
        }
    }
}
