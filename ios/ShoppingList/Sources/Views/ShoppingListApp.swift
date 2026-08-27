import SwiftUI

@main
struct ShoppingListApp: App {
    @State private var identity = Identity()
    /// Held for the life of the app, because it is a WCSession delegate: dropped, the
    /// watch's requests would arrive at nobody.
    @State private var watch: PhoneLink

    init() {
        let identity = Identity()
        _identity = State(initialValue: identity)
        _watch = State(initialValue: PhoneLink(token: { await identity.token() }))
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(identity)
                .task { await identity.restore() }
                // The sign-in flow leaves the app and comes back through this URL.
        }
    }
}

struct RootView: View {
    @Environment(Identity.self) private var identity

    /// Re-read after the address screen answers, because `ServerDirectory` is storage
    /// rather than observable state and nothing would otherwise tell SwiftUI.
    @State private var addressed = !ServerDirectory.needsAnAddress

    var body: some View {
        if !addressed {
            // Before sign-in and never after (C1). A development build has an address
            // from `Config.xcconfig`, so this screen does not appear there at all.
            ServerAddressView { address, _ in
                ServerDirectory.remember(address)
                addressed = true
            }
        } else {
            signedInOrNot
        }
    }

    @ViewBuilder
    private var signedInOrNot: some View {
        switch identity.state {
        case .unknown:
            ProgressView()
        case .signedOut:
            SignInView()
        case .signedIn:
            ListsView(
                api: API(
                    baseURL: Config.apiBaseURL,
                    token: { await identity.token() },
                    remembered: { identity.isRemembered }
                )
            )
        }
    }
}
