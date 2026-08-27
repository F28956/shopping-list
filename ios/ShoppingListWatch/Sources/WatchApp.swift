import SwiftUI

@main
struct ShoppingListWatchApp: App {
    /// Held for the life of the app, because it is the `WCSession` delegate: dropped,
    /// the phone's messages would arrive at nobody.
    @State private var store = WatchStore()

    var body: some Scene {
        WindowGroup {
            WatchListsView()
                .environment(store)
        }
    }
}
