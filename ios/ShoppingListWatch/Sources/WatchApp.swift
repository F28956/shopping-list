import SwiftUI

@main
struct ShoppingListWatchApp: App {
    /// Held for the life of the app, because it is the `WCSession` delegate: dropped,
    /// the phone's messages would arrive at nobody.
    @State private var store = WatchStore()

    init() {
        // Dropped unless somebody has turned logging on, like every `info` line. A watch
        // starts at `warn` whatever the phone said last: it stores no level of its own
        // deliberately -- one remembered on the wrist would outlive somebody turning
        // tracing off on the phone -- and picks the phone's out of the application
        // context a moment later, when `WatchStore` reads it.
        Log.info(.app, "the watch app started")
    }

    var body: some Scene {
        WindowGroup {
            WatchListsView()
                .environment(store)
        }
    }
}
