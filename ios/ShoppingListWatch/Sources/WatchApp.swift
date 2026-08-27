import SwiftUI

@main
struct ShoppingListWatchApp: App {
    @State private var identity = WatchIdentity()

    var body: some Scene {
        WindowGroup {
            WatchListsView(
                // Asked per request. A watch does not know its server until its phone
                // has told it, which happens in the same message as the token — and
                // that may be minutes after this app started.
                api: API(server: { Config.apiBaseURL }, token: { await identity.token() })
            )
            .environment(identity)
        }
    }
}
