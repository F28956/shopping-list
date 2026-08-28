import SwiftUI

@main
struct ShoppingListApp: App {
    @State private var identity = Identity()

    init() {
        let identity = Identity()
        _identity = State(initialValue: identity)

        // The phone is where the watch gets its config, and -- when there is no server
        // -- where its queue goes. See `WatchLink`. Started here rather than held as
        // state: it is a singleton because `WCSession` has one delegate, and a second
        // one would silently take over from the first.
        PhoneLink.shared.token = { await identity.token() }
        PhoneLink.shared.start()

        // Temporary, and here to be measured rather than used. Linking the device's
        // own server costs binary size, and the honest number only appears once the
        // linker has something to keep -- a library nothing calls is dead-stripped and
        // measures zero. See `LocalServer`; this line goes when a `Backend` conformer
        // takes its place.
        LocalServer.check()
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(identity)
                // Only when there is somewhere to sign in to. On a device kept to
                // itself there is nobody to restore and nothing that would use the
                // answer.
                .task {
                    if !ServerDirectory.isOnDeviceOnly { await identity.restore() }
                }
                // The sign-in flow leaves the app and comes back through this URL.
        }
    }
}

struct RootView: View {
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
                // question about hosting — so there is no first-run screen, no sheet
                // to dismiss, and nothing to sign in to. Somebody who has a server
                // goes and says so in settings.
                lists
            }
        }
        // Settings is the only thing that changes this, and it changes it under our
        // feet, so the answer is re-read rather than remembered from launch.
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            hasServer = !ServerDirectory.isOnDeviceOnly
        }
    }

    private var lists: some View {
        ListsView(
            api: API(
                baseURL: Config.apiBaseURL,
                token: { await identity.token() },
                remembered: { identity.isRemembered }
            )
        )
    }

    @ViewBuilder
    private var signedInOrNot: some View {
        switch identity.state {
        case .unknown:
            ProgressView()
        case .signedOut:
            SignInView()
        case .signedIn:
            lists
        }
    }
}
