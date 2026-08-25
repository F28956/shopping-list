import GoogleSignIn
import SwiftUI

@main
struct ShoppingListApp: App {
    @State private var identity = Identity()

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

/// Where the server is.
///
/// From the bundle rather than a constant, so pointing the app at a different machine
/// is a build setting — `localhost` is the phone itself, which is the first thing to
/// get wrong on a real device.
enum Config {
    static var apiBaseURL: URL {
        let raw = Bundle.main.object(forInfoDictionaryKey: "ShoppingListAPIBaseURL") as? String
        return URL(string: raw ?? "") ?? URL(string: "http://localhost:8080")!
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
