import SwiftUI

/// Which list, and nothing else.
///
/// Making, renaming, sharing and deleting lists are all missing on purpose. A watch is
/// glanced at with one hand full; it is the screen for what is left to get.
struct WatchListsView: View {
    @Environment(WatchStore.self) private var store

    private let cache = Cache.shared

    @State private var lists: [List] = []
    @State private var problem: Problem?
    @State private var loaded = false
    /// Whether the far end has ever answered. What is shown while this is false came
    /// out of the cache and may be old — and, crucially, is not evidence of an empty
    /// list.
    @State private var fresh = false

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty && store.mode == .unknown {
                    // Never been told anything, which is different from being told there
                    // is nothing. Saying "no lists" to somebody who has ten is worse
                    // than saying nothing.
                    ContentUnavailableView(
                        "Waiting for your phone",
                        systemImage: "iphone.radiowaves.left.and.right",
                        description: Text("Open Shopping on your phone once, and this fills in.")
                    )
                } else if let problem, lists.isEmpty {
                    // Only when there is nothing cached to show. Replacing a list this
                    // watch has seen with an error loses the one thing somebody came to
                    // look at, over a connection they cannot do anything about.
                    WatchProblemView(problem: problem) { Task { await load() } }
                } else if lists.isEmpty && fresh {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text("Make one on your phone.")
                    )
                } else {
                    SwiftUI.List {
                        ForEach(lists) { list in
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
                    WatchStatusDot(waiting: store.waiting, offline: problem != nil)
                }
            }
            .navigationDestination(for: List.self) { list in
                WatchItemsView(list: list, store: store)
            }
            .task {
                showWhatWeHave()
                await load()
            }
            // The phone's picture landing in the cache, which on a device with no
            // server is the only way a list ever changes here.
            .onReceive(NotificationCenter.default.publisher(for: .cacheChanged)) { _ in
                showWhatWeHave()
            }
        }
    }

    /// The last lists this watch saw, put up before anything is asked of anybody.
    private func showWhatWeHave() {
        let remembered = cache.lists()
        guard !remembered.isEmpty else {
            loaded = true
            return
        }
        lists = remembered
        loaded = true
    }

    /// Asks the server, when there is one.
    ///
    /// With no server there is nothing to ask: the lists arrive from the phone and are
    /// already in the cache. Trying anyway would be a request that can only ever fail,
    /// and an error on screen about a server nobody has.
    private func load() async {
        await store.send()

        guard store.fetches, let api = store.destination as? API else {
            fresh = store.heard
            loaded = true
            return
        }

        do {
            let answered = try await api.lists().items
            cache.remember(lists: answered)
            lists = answered
            problem = nil
            fresh = true
        } catch {
            problem = Problem(error)
        }
        loaded = true
    }
}
