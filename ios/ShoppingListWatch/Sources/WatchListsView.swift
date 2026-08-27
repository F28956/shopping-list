import SwiftUI

/// Which list, and nothing else.
///
/// Making, renaming, sharing and deleting lists are all missing on purpose. A watch is
/// glanced at with one hand full; it is the screen for what is left to get.
struct WatchListsView: View {
    @Environment(WatchStore.self) private var store

    var body: some View {
        NavigationStack {
            Group {
                if store.lists.isEmpty && !store.heard {
                    // Not an empty state: this watch has never been told anything, which
                    // is different from being told there is nothing. Saying "no lists"
                    // to somebody who has ten is worse than saying nothing.
                    ContentUnavailableView(
                        "Waiting for your phone",
                        systemImage: "iphone.radiowaves.left.and.right",
                        description: Text("Open Shopping on your phone once, and this fills in.")
                    )
                } else if store.lists.isEmpty {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text("Make one on your phone.")
                    )
                } else {
                    SwiftUI.List {
                        ForEach(store.lists) { list in
                            NavigationLink(value: list) {
                                Text(list.name)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Lists")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    WatchStatusDot(waiting: store.waiting, onDeviceOnly: store.onDeviceOnly)
                }
            }
            .navigationDestination(for: WatchLink.ListOnTheWatch.self) { list in
                WatchItemsView(list: list)
            }
        }
    }
}
