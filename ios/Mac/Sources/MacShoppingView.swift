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

    /// Everything about the lists themselves — see `ListsModel`, which the phone
    /// shares. It used to be a second copy of it here, and the copy was three fixes
    /// behind: a list could not be made with no server, lists made offline would have
    /// appeared twice, and somebody this server will not have got a raw dialog.
    @State private var model: ListsModel

    // What is genuinely this window's: which list is selected, and which sheet is open.
    @State private var chosen: List.ID?
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?
    @State private var sharing: List?
    @State private var joining = false
    /// There is no server. The default -- see `ServerDirectory`. Re-read when settings
    /// change the answer, because storage is not observable state.
    @State private var onDeviceOnly = ServerDirectory.isOnDeviceOnly

    init(api: API) {
        self.api = api
        _model = State(initialValue: ListsModel(api: api))
    }

    private var selected: List? { model.lists.first { $0.id == chosen } }

    var body: some View {
        NavigationSplitView {
            Group {
                if !model.loaded {
                    ProgressView()
                } else if model.lists.isEmpty && !model.fresh && !onDeviceOnly {
                    // Before the empty state: after any failed load with nothing
                    // cached, "No lists" is an emptiness nobody has verified. Only a
                    // server that answered can earn the empty state.
                    //
                    // Except on a Mac kept to itself, where there is no server to have
                    // checked with and this machine is the only thing that could know.
                    // There, empty means empty -- and saying "Can't reach the server"
                    // to somebody who chose not to have one is the app complaining
                    // about a decision they made on purpose.
                    ContentUnavailableView(
                        model.offline ? "Can't reach the server" : "Couldn't load your lists",
                        systemImage: model.offline ? "icloud.slash" : "exclamationmark.triangle",
                        description: Text(
                            model.offline
                                ? "Your lists will appear as soon as there is a connection."
                                : "Whether you have any is not known yet."
                        )
                    )
                } else if model.lists.isEmpty {
                    ContentUnavailableView(
                        "No lists",
                        systemImage: "cart",
                        description: Text(
                            onDeviceOnly
                                ? "Make one with the button above. It stays on this Mac."
                                : "Make one in the browser and it appears here."
                        )
                    )
                } else {
                    SwiftUI.List(selection: $chosen) {
                        // Nothing to say on a Mac with no server: nothing is stale,
                        // because there is nowhere it could have gone stale against.
                        if model.offline && !onDeviceOnly {
                            OfflineNote()
                        }

                        ForEach(model.lists) { list in
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

                        if model.truncated {
                            Text("Showing \(model.lists.count) of \(Int(model.total)).")
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
            ShareSheet(list: list, api: api) { await model.load() }
        }
        .sheet(isPresented: $joining) {
            JoinSheet { found in
                await model.attempt {
                    let joined = try await api.join(withToken: found)
                    await model.load()
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
                    // Through the model, which queues it when there is no server to
                    // ask. This used to call `api.createList` directly, so on a Mac
                    // deliberately kept off a server the one button in the toolbar
                    // raised a dialog and made nothing.
                    //
                    // Selected on arrival either way: making a list is how you say
                    // which one you want to be looking at.
                    if let made = await model.makeList(named: name) {
                        chosen = made.id
                    }
                case .rename(let list):
                    await model.attempt { try await api.rename(list, to: name) }
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
                    await model.attempt { try await api.delete(list) }
                    // The detail pane is about a list that has gone.
                    if chosen == list.id { chosen = model.lists.first?.id }
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
        // One place that decides what is selected, rather than a line in each of the
        // two functions that can change the lists. A selection pointing at a list that
        // has gone shows an empty detail pane with no way back to a full one, and
        // opening on nothing wastes the width the split view exists for.
        .onChange(of: model.lists, initial: true) {
            if chosen == nil || !model.lists.contains(where: { $0.id == chosen }) {
                chosen = model.lists.first?.id
            }
        }
        .task {
            // Set here rather than passed in: the identity is an environment value and
            // a view has none of those when its state is built. See `ListsModel`.
            model.signedOut = { because in
                if let because {
                    identity.signOut(because: because)
                } else {
                    identity.signOut()
                }
            }
            model.showWhatWeHave()
            await model.load()
        }
        .task { await model.watchLists() }
        .alert("Could not load", isPresented: .constant(model.error != nil)) {
            Button("OK") { model.error = nil }
        } message: {
            Text(model.error ?? "")
        }
    }
}
