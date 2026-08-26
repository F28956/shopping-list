import SwiftUI

/// Lists on the left, what is on one on the right.
///
/// A split view rather than the phone's push-and-pop: a Mac has the width to show
/// which list you are in while you are in it, and losing that was the only thing the
/// small screen forced.
struct MacShoppingView: View {
    let api: API
    @Environment(Identity.self) private var identity

    @State private var lists: [List] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var chosen: List.ID?
    @State private var error: String?
    @State private var loaded = false
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?

    private var selected: List? { lists.first { $0.id == chosen } }

    var body: some View {
        NavigationSplitView {
            Group {
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text("Make one in the browser and it appears here.")
                    )
                } else {
                    SwiftUI.List(selection: $chosen) {
                        ForEach(lists) { list in
                            HStack {
                                Text(list.name)
                                Spacer()
                                // Said, not implied: a list you may only read looks
                                // exactly like one you own until you try to change it.
                                if !list.mayEdit {
                                    Image(systemName: "eye")
                                        .foregroundStyle(.secondary)
                                        .help("You can read this list, not change it")
                                }
                            }
                            .tag(list.id)
                            .accessibilityIdentifier("list.\(list.name)")
                            .contextMenu {
                                // Renaming and deleting are the owner's, not an
                                // editor's: an editor was given a list, not the say
                                // over whether it exists.
                                if list.role >= .owner {
                                    Button("Rename…") { naming = .rename(list) }
                                    Divider()
                                    Button("Delete…", role: .destructive) {
                                        deleting = list
                                    }
                                }
                            }
                        }

                        if truncated {
                            Text("Showing \(lists.count) of \(Int(total)).")
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 220)
            .navigationTitle("Lists")
        } detail: {
            if let selected {
                MacItemsView(api: api, list: selected)
                    // Rebuilt when the choice changes: the screen is about one list,
                    // and carrying the previous one's state into it is how a tick
                    // lands on the wrong row.
                    .id(selected.id)
            } else {
                ContentUnavailableView(
                    "Pick a list",
                    systemImage: "sidebar.left",
                    description: Text("Choose one on the left.")
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button {
                    naming = .create
                } label: {
                    Label("New list", systemImage: "plus")
                }
                .help("New list")
                .accessibilityIdentifier("list.new")
            }
            ToolbarItem(placement: .primaryAction) {
                Button("Sign out") { identity.signOut() }
            }
        }
        .sheet(item: $naming) { purpose in
            ListNameSheet(purpose: purpose) { name in
                switch purpose {
                case .create:
                    await attempt {
                        // Selected on arrival: making a list is how you say which one
                        // you want to be looking at.
                        let made = try await api.createList(named: name)
                        await load()
                        chosen = made.id
                    }
                case .rename(let list):
                    await attempt { try await api.rename(list, to: name) }
                }
            }
        }
        .confirmationDialog(
            "Delete \(deleting?.name ?? "this list")?",
            isPresented: .constant(deleting != nil),
            presenting: deleting
        ) { list in
            Button("Delete", role: .destructive) {
                deleting = nil
                Task {
                    await attempt { try await api.delete(list) }
                    // The detail pane is about a list that has gone.
                    if chosen == list.id { chosen = lists.first?.id }
                }
            }
            .accessibilityIdentifier("delete.confirm")
            Button("Cancel", role: .cancel) { deleting = nil }
                .accessibilityIdentifier("delete.cancel")
        } message: { _ in
            Text("Everything on it goes too. This cannot be undone.")
        }
        .task { await load() }
        .alert("Could not load", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    /// Runs something that changes the lists, then reloads.
    private func attempt(_ work: () async throws -> Void) async {
        do {
            try await work()
            await load()
        } catch let problem as APIError {
            if case .unauthorized = problem {
                identity.signOut()
            } else {
                error = problem.localizedDescription
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func load() async {
        do {
            let listing = try await api.lists()
            lists = listing.items
            total = listing.total
            truncated = listing.truncated
            // Opening on nothing wastes the width the split view exists for -- and
            // a selection pointing at a list that has gone shows an empty detail
            // pane with no way back to a full one.
            if chosen == nil || !lists.contains(where: { $0.id == chosen }) {
                chosen = lists.first?.id
            }
            error = nil
        } catch let problem as APIError {
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
