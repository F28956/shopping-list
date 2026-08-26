import SwiftUI

/// The lists this person can see — the ones they own and the ones shared with them.
///
/// Making, renaming, deleting, sharing and joining all happen here now. Each is
/// reachable two ways: a swipe for anyone who knows the gesture, and a long press for
/// everyone else, because a swipe rewards only somebody who already guessed it was
/// there.
struct ListsView: View {
    let api: API
    @Environment(Identity.self) private var identity

    private let cache = Cache.shared

    @State private var lists: [List] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var error: String?
    @State private var loaded = false
    /// The server could not be reached last time we asked. Not an error and not worth
    /// a dialog -- but the difference between "you have no lists" and "I could not
    /// find out" has to reach the screen.
    @State private var offline = false
    /// Whether the server has ever answered. What is shown while this is false came
    /// out of the cache and may be old.
    @State private var fresh = false
    /// Guards against a drain and a reload calling each other round in a circle.
    @State private var draining = false
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?
    @State private var sharing: List?
    @State private var joining = false

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty && !fresh {
                    // Before the empty state, and the order is the point: this app
                    // used to say "No lists" whenever a load failed and there was
                    // nothing cached -- an emptiness it had never verified. `fresh`
                    // is the only thing that earns the empty state, and only the
                    // server can set it. Losing signal afterwards does not unsay it.
                    ContentUnavailableView {
                        Label(
                            offline ? "Can't reach the server" : "Couldn't load your lists",
                            systemImage: offline ? "icloud.slash" : "exclamationmark.triangle"
                        )
                    } description: {
                        Text(
                            offline
                                ? "Your lists will appear as soon as there is a connection."
                                : "Whether you have any is not known yet."
                        )
                    } actions: {
                        Button("Try again") { Task { await load() } }
                    }
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
                        if offline {
                            OfflineNote()
                        }

                        ForEach(lists) { list in
                            NavigationLink(value: list) {
                                Text(list.name)
                            }
                            // Renaming and deleting are the owner's. An editor was
                            // given a list, not the say over whether it exists.
                            .contextMenu {
                                Button("Share…", systemImage: "person.badge.plus") {
                                    sharing = list
                                }
                                if list.role >= .owner {
                                    Button("Rename…", systemImage: "pencil") {
                                        naming = .rename(list)
                                    }
                                    Divider()
                                    Button("Delete…", systemImage: "trash", role: .destructive) {
                                        deleting = list
                                    }
                                }
                            }
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
                    Button("Sign out") {
                        // What is cached belongs to whoever is signing out. The next
                        // person to use this device is a different person.
                        cache.forgetEverything()
                        identity.signOut()
                    }
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
            .task {
                showWhatWeHave()
                await load()
            }
            .task { await watchLists() }
            .alert("Could not load", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
    }


    /// Keeps the sidebar in step with lists made, renamed, deleted or joined anywhere.
    ///
    /// A list's own stream cannot carry this: one that has just been made has no
    /// watchers at all, which is why a list created on a phone never appeared here.
    private func watchLists() async {
        var reconnecting = false

        while !Task.isCancelled {
            if reconnecting { await load() }

            do {
                for try await _ in try await api.listChanges() {
                    await load()
                }
            } catch let problem as APIError {
                if case .unauthorized = problem {
                    identity.signOut()
                    return
                }
                // A refusal is not a dropped connection. Reconnecting every three
                // seconds to be refused again is a loop that ends only when somebody
                // closes the app, and each turn of it used to raise another dialog.
                if case .forbidden = problem { return }
                if case .notAdmitted = problem {
                    identity.signOut(because: problem.localizedDescription)
                    return
                }
            } catch {}

            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
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
            } else if case .notAdmitted = problem {
                identity.signOut(because: problem.localizedDescription)
            } else {
                error = problem.localizedDescription
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Puts the last-loaded lists up before asking the server anything.
    ///
    /// The screen is never blank while a request is in flight, and on a phone with no
    /// signal it is never blank at all. Guarded on `fresh` so a slow disk read cannot
    /// land after a fast answer and put yesterday's lists back.
    private func showWhatWeHave() {
        guard !fresh else { return }
        let remembered = cache.lists()
        guard !remembered.isEmpty else { return }
        lists = remembered
        total = Int64(remembered.count)
        loaded = true
    }

    /// Empties the outbox, wherever its contents belong.
    ///
    /// A change queued on any list goes: the operation carries the list it was made
    /// against, so nothing here needs to know which screen it came from. Failures are
    /// the outbox's business -- see ``Outbox/drain(through:)`` -- and what is left
    /// stays queued for the next successful load.
    private func sendQueued() async {
        guard !draining, cache.outbox.waiting > 0 else { return }
        draining = true
        _ = await cache.outbox.drain(through: api)
        draining = false
    }

    private func load() async {
        do {
            let listing = try await api.lists()
            cache.remember(lists: listing.items)
            lists = listing.items
            total = listing.total
            truncated = listing.truncated
            error = nil
            offline = false
            fresh = true
            // The server is reachable, so anything queued anywhere goes now.
            //
            // Here as well as on the list screen, because the app opens here: a phone
            // that came out of a shop and was put in a pocket would otherwise hold its
            // ticks until somebody happened to open the list they were made on.
            await sendQueued()
        } catch let problem as APIError {
            // A signed-out session is not an error worth a dialog: the root view puts
            // the sign-in screen back as soon as the state changes.
            if case .unauthorized = problem {
                identity.signOut()
            } else if case .notAdmitted = problem {
                // Not a person with an empty list -- a person this server will not
                // have. The sign-in screen is where that is said, and it is said once
                // rather than raised again every time the stream reconnects.
                identity.signOut(because: problem.localizedDescription)
            } else if case .transport = problem {
                // Not shown. Being out of signal is a state, not an event: a phone in
                // a basement would raise this every few seconds, and a dialog for each
                // is an interruption on top of an app that is still usable.
                offline = true
                if !fresh { showWhatWeHave() }
            } else {
                error = problem.localizedDescription
            }
        } catch {
            self.error = error.localizedDescription
        }
        loaded = true
    }
}
