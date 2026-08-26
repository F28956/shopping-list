import SwiftUI

/// Which list, and nothing else.
///
/// Making, renaming, sharing and deleting lists are all missing on purpose. A watch
/// is glanced at with one hand full; it is the screen for what is left to get.
struct WatchListsView: View {
    let api: API
    @Environment(WatchIdentity.self) private var identity

    @State private var lists: [List] = []
    @State private var problem: Problem?
    @State private var loaded = false
    /// Whether the server has ever answered. What is shown while this is false came out
    /// of the cache and may be old — and, crucially, is not evidence of an empty list.
    @State private var fresh = false
    @State private var draining = false
    /// How many changes are waiting, anywhere. The lists screen is where the app opens,
    /// so it is where somebody first sees whether the watch is in step.
    @State private var queued = 0

    private let cache = Cache.shared

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else if let problem, lists.isEmpty {
                    // Only when there is nothing cached to show. Replacing a list this
                    // watch has seen with an error loses the one thing somebody came to
                    // look at, over a connection they cannot do anything about.
                    WatchProblemView(problem: problem) { Task { await load() } }
                } else if lists.isEmpty && fresh {
                    // `fresh` is what earns this: only a server that answered can say
                    // somebody has no lists.
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
                    WatchStatusDot(waiting: queued, offline: problem != nil)
                }
            }
            .navigationDestination(for: List.self) { list in
                WatchItemsView(api: api, list: list)
            }
            .task {
                showWhatWeHave()
                await load()
            }
            .task {
                // Cheap, and the only way the dot on this screen stays honest while
                // somebody is looking at it.
                while !Task.isCancelled {
                    queued = cache.outbox.waiting
                    try? await Task.sleep(for: .seconds(2))
                }
            }
        }
    }

    /// The last lists this watch saw, put up before anything is asked of the server.
    private func showWhatWeHave() {
        guard !fresh else { return }
        let remembered = cache.lists()
        guard !remembered.isEmpty else { return }
        lists = remembered
        loaded = true
    }

    private func load() async {
        do {
            let answered = try await api.lists().items
            cache.remember(lists: answered)
            lists = answered
            problem = nil
            fresh = true
            await sendQueued()
        } catch {
            problem = Problem(error, identity: identity)
        }
        loaded = true
    }

    /// Empties the outbox, wherever its contents belong.
    ///
    /// Here as well as on the list screen, because the app opens here: a watch that came
    /// out of a shop would otherwise hold its ticks until somebody opened the list they
    /// were made on.
    private func sendQueued() async {
        guard !draining, cache.outbox.waiting > 0 else { return }
        draining = true
        _ = await cache.outbox.drain(through: api)
        draining = false
        queued = cache.outbox.waiting
    }
}
