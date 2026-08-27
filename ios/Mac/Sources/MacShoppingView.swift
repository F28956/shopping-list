import SwiftUI

/// Lists on the left, what is on one on the right.
///
/// A split view rather than the phone's push-and-pop: a Mac has the width to show
/// which list you are in while you are in it, and losing that was the only thing the
/// small screen forced.
struct MacShoppingView: View {
    let api: API
    @Environment(Identity.self) private var identity

    private let cache = Cache.shared

    @State private var lists: [List] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var chosen: List.ID?
    @State private var error: String?
    @State private var loaded = false
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?
    @State private var sharing: List?
    @State private var joining = false
    /// There is no server. The default -- see `ServerDirectory`. Re-read when settings
    /// change the answer, because storage is not observable state.
    @State private var onDeviceOnly = ServerDirectory.isOnDeviceOnly
    /// See `ListsView.offline` on the phone: the same two flags, for the same reason.
    @State private var offline = false
    @State private var fresh = false
    /// Guards against a drain and a reload calling each other round in a circle.
    @State private var draining = false
    /// How many changes are waiting, anywhere. The window opens here, so this is where
    /// somebody first sees whether the Mac is in step.
    @State private var queued = 0

    private var selected: List? { lists.first { $0.id == chosen } }

    var body: some View {
        NavigationSplitView {
            Group {
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty && !fresh {
                    // Before the empty state: after any failed load with nothing
                    // cached, "No lists" is an emptiness nobody has verified. Only a
                    // server that answered can earn the empty state.
                    ContentUnavailableView(
                        offline ? "Can't reach the server" : "Couldn't load your lists",
                        systemImage: offline ? "icloud.slash" : "exclamationmark.triangle",
                        description: Text(
                            offline
                                ? "Your lists will appear as soon as there is a connection."
                                : "Whether you have any is not known yet."
                        )
                    )
                } else if lists.isEmpty {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text("Make one in the browser and it appears here.")
                    )
                } else {
                    SwiftUI.List(selection: $chosen) {
                        if offline {
                            OfflineNote()
                        }

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
                                // A share link names a server. With none there is no
                                // link to make, so the option is absent rather than
                                // present and failing.
                                if !onDeviceOnly {
                                    Button("Share…") { sharing = list }
                                    Divider()
                                }
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
            // Joining is somebody else's list on somebody's server, and signing out
            // needs somebody signed in. With no server there is neither, so both are
            // absent rather than present and refusing.
            if !onDeviceOnly {
                ToolbarItem(placement: .navigation) {
                    Button {
                        joining = true
                    } label: {
                        Label("Join a list", systemImage: "person.badge.plus")
                    }
                    .help("Join a list somebody shared with you")
                    .accessibilityIdentifier("list.join")
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Sign out") {
                        // See the phone: cached shopping belongs to whoever signed in.
                        cache.forgetEverything()
                        identity.signOut()
                    }
                }
            }
        }
        .sheet(item: $sharing) { list in
            ShareSheet(list: list, api: api) { await load() }
        }
        .sheet(isPresented: $joining) {
            JoinSheet { found in
                await attempt {
                    let joined = try await api.join(withToken: found)
                    await load()
                    // Opened on arrival: following a link is how you say which list
                    // you want to be looking at.
                    chosen = joined.id
                }
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
        // Settings is the only thing that changes this, and it changes it under our
        // feet, so the answer is re-read rather than remembered from launch.
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            onDeviceOnly = ServerDirectory.isOnDeviceOnly
        }
        .task {
            showWhatWeHave()
            await load()
        }
        .task {
            // Cheap, and the only way the dot stays honest while somebody is looking
            // at it: every items view drains the same queue.
            while !Task.isCancelled {
                queued = cache.outbox.waiting
                try? await Task.sleep(for: .seconds(2))
            }
        }
        .task { await watchLists() }
        .alert("Could not load", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
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
                // Reconnecting every three seconds to be refused again is a loop
                // nothing ends, and each turn of it raised another dialog.
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
                // A person this server will not have is not a person with an empty
                // list. Said on the sign-in screen, once.
                identity.signOut(because: problem.localizedDescription)
            } else {
                error = problem.localizedDescription
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Puts the last-loaded lists up before asking the server anything -- see the
    /// phone's `ListsView.showWhatWeHave`.
    private func showWhatWeHave() {
        guard !fresh else { return }
        let remembered = cache.lists()
        guard !remembered.isEmpty else { return }
        lists = remembered
        total = Int64(remembered.count)
        if chosen == nil { chosen = remembered.first?.id }
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
        queued = cache.outbox.waiting
    }

    private func load() async {
        do {
            let listing = try await api.lists()
            cache.remember(lists: listing.items)
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
            offline = false
            fresh = true
            // The server is reachable, so anything queued anywhere goes now.
            //
            // Here as well as on the list screen, because the app opens here: a phone
            // that came out of a shop and was put in a pocket would otherwise hold its
            // ticks until somebody happened to open the list they were made on.
            await sendQueued()
        } catch let problem as APIError {
            if case .unauthorized = problem {
                identity.signOut()
            } else if case .transport = problem {
                // No signal is a state, not an event -- see the phone's ListsView.
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
