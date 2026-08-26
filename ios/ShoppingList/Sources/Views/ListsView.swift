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
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?
    @State private var sharing: List?
    @State private var joining = false

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty {
                    ContentUnavailableView {
                        Label("No lists", systemImage: "cart")
                    } description: {
                        Text("Make one to get started.")
                    } actions: {
                        Button("New list") { naming = .create }
                            .accessibilityIdentifier("list.new.empty")
                    }
                } else {
                    SwiftUI.List {
                        ForEach(lists) { list in
                            NavigationLink(value: list) {
                                Text(list.name)
                            }
                            // Renaming and deleting are the owner's. An editor was
                            // given a list, not the say over whether it exists.
                            .swipeActions(edge: .leading) {
                                Button {
                                    sharing = list
                                } label: {
                                    Label("Share", systemImage: "person.badge.plus")
                                }
                                .tint(.accentColor)
                            }
                            .swipeActions(edge: .trailing) {
                                if list.role >= .owner {
                                    Button(role: .destructive) {
                                        deleting = list
                                    } label: {
                                        Label("Delete", systemImage: "trash")
                                    }

                                    Button {
                                        naming = .rename(list)
                                    } label: {
                                        Label("Rename", systemImage: "pencil")
                                    }
                                    .tint(.accentColor)
                                }
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
                ToolbarItem(placement: .topBarLeading) {
                    Button("Sign out") { identity.signOut() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button("New list", systemImage: "plus") { naming = .create }
                        Button("Join a list", systemImage: "person.badge.plus") {
                            joining = true
                        }
                    } label: {
                        Label("Add", systemImage: "plus")
                    }
                    .accessibilityIdentifier("list.new")
                }
            }
            .sheet(item: $sharing) { list in
                ShareSheet(list: list, api: api) { await load() }
            }
            .sheet(isPresented: $joining) {
                JoinSheet { found in
                    await attempt { _ = try await api.join(withToken: found) }
                }
                .presentationDetents([.height(240)])
            }
            .sheet(item: $naming) { purpose in
                ListNameSheet(purpose: purpose) { name in
                    switch purpose {
                    case .create:
                        await attempt { try await api.createList(named: name) }
                    case .rename(let list):
                        await attempt { try await api.rename(list, to: name) }
                    }
                }
                .presentationDetents([.height(200)])
            }
            .confirmationDialog(
                "Delete \(deleting?.name ?? "this list")?",
                isPresented: .constant(deleting != nil),
                titleVisibility: .visible,
                presenting: deleting
            ) { list in
                Button("Delete", role: .destructive) {
                    deleting = nil
                    Task { await attempt { try await api.delete(list) } }
                }
                Button("Cancel", role: .cancel) { deleting = nil }
            } message: { _ in
                Text("Everything on it goes too. This cannot be undone.")
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
