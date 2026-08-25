import SwiftUI

@main
struct ShoppingListWatchApp: App {
    @State private var identity = WatchIdentity()

    var body: some Scene {
        WindowGroup {
            WatchListsView(
                api: API(baseURL: Config.apiBaseURL, token: { await identity.token() })
            )
            .environment(identity)
        }
    }
}
