import GoogleSignIn
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
                .onOpenURL { GIDSignIn.sharedInstance.handle($0) }
        }
    }
}

struct RootView: View {
    @Environment(Identity.self) private var identity

    var body: some View {
        switch identity.state {
        case .unknown:
            ProgressView()
        case .signedOut:
            SignInView()
        case .signedIn:
            ListsView(api: API(baseURL: Config.apiBaseURL, token: { await identity.token() }))
        }
    }
}
