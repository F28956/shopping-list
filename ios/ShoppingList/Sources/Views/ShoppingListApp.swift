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
            ServerAddressView(
                accepted: { address, _ in
                    ServerDirectory.remember(address)
                    addressed = true
                },
                declined: {
                    ServerDirectory.onlyThisDevice()
                    addressed = true
                }
            )
        } else if ServerDirectory.isOnDeviceOnly {
            // S1. No server means nobody to sign in to, so there is no sign-in. The
            // app runs exactly as it does with no signal — which is not a compromise
            // but the point: `API` fails every call as a transport error, the cache
            // answers, and the outbox keeps what was written down until there is
            // somewhere to send it.
            lists
        } else {
            signedInOrNot
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
