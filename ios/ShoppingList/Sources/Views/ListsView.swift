import SwiftUI

/// The lists this person can see — the ones they own and the ones shared with them.
///
/// Read-only: making, renaming, deleting and sharing lists are things you do sitting
/// down, and they are in the web UI. This is the screen you open in a shop.
struct ListsView: View {
    let api: API
    @Environment(Identity.self) private var identity

    @State private var lists: [List] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var error: String?
    @State private var loaded = false

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text("Make one in the browser and it will appear here.")
                    )
                } else {
                    SwiftUI.List {
                        ForEach(lists) { list in
                            NavigationLink(value: list) {
                                Text(list.name)
                            }
                        }

                        // The lists that did not fit are not missing, and saying so
                        // is the difference between "elsewhere" and "deleted".
                        if truncated {
                            Text("Showing \(lists.count) of \(total).")
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .navigationTitle("Lists")
            .navigationDestination(for: List.self) { list in
                ItemsView(api: api, list: list)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Sign out") { identity.signOut() }
                }
            }
            .refreshable { await load() }
            .task { await load() }
            .alert("Could not load", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
    }

    private func load() async {
        do {
            let listing = try await api.lists()
            lists = listing.items
            total = listing.total
            truncated = listing.truncated
            error = nil
        } catch let problem as APIError {
            // A signed-out session is not an error worth a dialog: the root view puts
            // the sign-in screen back as soon as the state changes.
            if case .unauthorized = problem {
                identity.signOut()
            } else {
                error = problem.localizedDescription
            }
        } catch {
            self.error = error.localizedDescription
        }
        loaded = true
    }
}
