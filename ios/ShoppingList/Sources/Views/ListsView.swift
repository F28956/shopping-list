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

    /// Everything about the lists themselves — see `ListsModel`, which the Mac shares.
    @State private var model: ListsModel

    // What is genuinely this screen's: which sheet is open, and what is being acted
    // on. The rest is the model's.
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?
    @State private var sharing: List?
    @State private var joining = false
    /// There is no server, because somebody said so on the first screen. Read once:
    /// it only changes by leaving this screen entirely.
    private let onDeviceOnly = ServerDirectory.isOnDeviceOnly

    init(api: API) {
        self.api = api
        _model = State(initialValue: ListsModel(api: api))
    }
    /// The two screens behind the menu, as one piece of state.
    ///
    /// Two `.sheet(isPresented:)` modifiers on the same view is a long-standing
    /// SwiftUI trap: only one of them is honoured, silently, and the other button
    /// does nothing. One `.sheet(item:)` cannot have that problem.
    private enum Elsewhere: String, Identifiable {
        case settings

        var id: String { rawValue }
    }

    @State private var elsewhere: Elsewhere?

    /// The one action this screen has.
    private var newListButton: some View {
        Button {
            naming = .create
        } label: {
            Image(systemName: "plus")
                .font(.title2.weight(.semibold))
                .frame(width: 56, height: 56)
        }
        .background(.tint, in: Circle())
        .foregroundStyle(.white)
        .shadow(radius: 4, y: 2)
        .padding(20)
        .accessibilityLabel("New list")
        .accessibilityIdentifier("list.new")
    }

    var body: some View {
        NavigationStack {
            Group {
                if !model.loaded {
                    ProgressView()
                } else if model.lists.isEmpty && !model.fresh && !onDeviceOnly {
                    // Before the empty state, and the order is the point: this app
                    // used to say "No lists" whenever a load failed and there was
                    // nothing cached -- an emptiness it had never verified. `fresh`
                    // is the only thing that earns the empty state, and only the
                    // server can set it. Losing signal afterwards does not unsay it.
                    //
                    // Except with no server, where nothing can ever set `fresh` and
                    // this device is the only thing that could know. There, empty
                    // means empty, and the ordinary empty state below is the right
                    // one -- it already offers to make a list, which is the only
                    // thing to do about it.
                    ContentUnavailableView {
                        Label(
                            model.offline ? "Can't reach the server" : "Couldn't load your lists",
                            systemImage: model.offline ? "icloud.slash" : "exclamationmark.triangle"
                        )
                    } description: {
                        Text(
                            model.offline
                                ? "Your lists will appear as soon as there is a connection."
                                : "Whether you have any is not known yet."
                        )
                    } actions: {
                        Button("Try again") { Task { await model.load() } }
                    }
                } else if model.lists.isEmpty {
                    // No action here. There is a button in the corner already, and a
                    // second one is the same thing twice on a screen with nothing else
                    // on it.
                    ContentUnavailableView {
                        Label("No lists", systemImage: "cart")
                    } description: {
                        Text(
                            onDeviceOnly
                                ? "Make one with the button below. It stays on this phone."
                                : "Make one with the button below to get started."
                        )
                    }
                } else {
                    SwiftUI.List {
                        if model.offline && !onDeviceOnly {
                            OfflineNote()
                        }

                        ForEach(model.lists) { list in
                            NavigationLink(value: list) {
                                Text(list.name)
                            }
                            // Renaming and deleting are the owner's. An editor was
                            // given a list, not the say over whether it exists.
                            .contextMenu {
                                // Sharing is the mirror of joining: a share link names
                                // a server, and with no server there is no link to
                                // make. Absent rather than present and failing.
                                if !onDeviceOnly {
                                    Button("Share…", systemImage: "person.badge.plus") {
                                        sharing = list
                                    }
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
                                if !onDeviceOnly {
                                    Button {
                                        sharing = list
                                    } label: {
                                        Label("Share", systemImage: "person.badge.plus")
                                    }
                                    .tint(.accentColor)
                                }
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
                        if model.truncated {
                            Text("Showing \(model.lists.count) of \(model.total).")
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
            // Making a list is the one thing this screen is for, so it gets a button
            // of its own rather than a line in a menu — and it sits where a thumb
            // already is rather than at the top of a tall phone.
            .overlay(alignment: .bottomTrailing) { newListButton }
            .toolbar {
                // The item and not just its contents: a toolbar item draws its own
                // background on iOS 26, so an empty dot would leave a chip with
                // nothing in it.
                if !onDeviceOnly {
                    ToolbarItem(placement: .topBarLeading) {
                        StatusDot(waiting: model.waiting, offline: model.offline)
                    }
                }
                ToolbarItem(placement: .topBarLeading) {
                    // Nobody is signed in on a device kept to itself, so there is
                    // nobody to sign out. Offering it would be a button that throws
                    // away somebody's only copy of their shopping and calls it
                    // leaving.
                    if !onDeviceOnly {
                        Button("Sign out") {
                            // What is cached belongs to whoever is signing out. The
                            // next person to use this device is a different person.
                            cache.forgetEverything()
                            identity.signOut()
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Settings", systemImage: "gear") { elsewhere = .settings }
                        .accessibilityIdentifier("settings")
                }
            }
            .sheet(item: $elsewhere) { screen in
                switch screen {
                case .settings:
                    SettingsView(
                        cache: cache,
                        api: api,
                        isOwner: model.isOwner,
                        joinAList: { joining = true }
                    )
                }
            }
            .sheet(item: $sharing) { list in
                ShareSheet(list: list, api: api) { await model.load() }
            }
            .sheet(isPresented: $joining) {
                JoinSheet { found in
                    await model.attempt { _ = try await api.join(withToken: found) }
                }
                .presentationDetents([.height(240)])
            }
            .sheet(item: $naming) { purpose in
                ListNameSheet(purpose: purpose) { name in
                    switch purpose {
                    case .create:
                        await model.makeList(named: name)
                    case .rename(let list):
                        await model.attempt { try await api.rename(list, to: name) }
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
                    Task { await model.attempt { try await api.delete(list) } }
                }
                Button("Cancel", role: .cancel) { deleting = nil }
            } message: { _ in
                Text("Everything on it goes too. This cannot be undone.")
            }
            .refreshable { await model.load() }
            .task {
                // Set here rather than passed in: the identity is an environment value
                // and a view has none of those when its state is built. See
                // `ListsModel`.
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
}
