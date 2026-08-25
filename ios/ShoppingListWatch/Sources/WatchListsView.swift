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

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else if let problem {
                    WatchProblemView(problem: problem) { Task { await load() } }
                } else if lists.isEmpty {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text("Make one on your phone.")
                    )
                } else {
                    SwiftUI.List(lists) { list in
                        NavigationLink(value: list) {
                            Text(list.name)
                        }
                    }
                }
            }
            .navigationTitle("Lists")
            .navigationDestination(for: List.self) { list in
                WatchItemsView(api: api, list: list)
            }
            .task { await load() }
        }
    }

    private func load() async {
        loaded = false
        do {
            lists = try await api.lists()
            problem = nil
        } catch {
            problem = Problem(error, identity: identity)
        }
        loaded = true
    }
}
