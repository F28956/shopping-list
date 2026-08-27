import AuthenticationServices
import SwiftUI

@main
struct ShoppingListMacApp: App {
    @State private var identity = Identity()

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
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            hasServer = !ServerDirectory.isOnDeviceOnly
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
    private var shopping: some View {
        MacShoppingView(
            api: API(
                // With no server this is a placeholder that refuses every connection,
                // which is the point: the failure is a transport failure, and the app
                // already knows how to queue through one of those.
                baseURL: Config.apiBaseURL,
                token: { await identity.token() },
                remembered: { identity.isRemembered }
            )
        )
    }
}

struct MacSignInView: View {
    @Environment(Identity.self) private var identity
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(spacing: 16) {
            Text("Shopping list")
                .font(.largeTitle.weight(.semibold))
            Text("The same lists as the phone, with a keyboard.")
                .foregroundStyle(.secondary)

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
        }
        .padding(40)
    }

}
