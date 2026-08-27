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
    /// How many changes are waiting, anywhere. The lists screen is where the app opens,
    /// so it is where somebody first sees whether this device is in step.
    @State private var queued = 0
    @State private var naming: ListNameSheet.Purpose?
    @State private var deleting: List?
    @State private var sharing: List?
    @State private var joining = false
    /// Whether this person administers this server, which decides whether the screen
    /// that manages it exists. Hiding it is a courtesy: every route behind it is
    /// refused in the service layer to anybody else.
    /// There is no server, because somebody said so on the first screen. Read once:
    /// it only changes by leaving this screen entirely.
    private let onDeviceOnly = ServerDirectory.isOnDeviceOnly
    @State private var isOwner = false
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
                if !loaded {
                    ProgressView()
                } else if lists.isEmpty && !fresh && !onDeviceOnly {
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
                        if offline && !onDeviceOnly {
                            OfflineNote()
                        }

                        ForEach(lists) { list in
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
            // Making a list is the one thing this screen is for, so it gets a button
            // of its own rather than a line in a menu — and it sits where a thumb
            // already is rather than at the top of a tall phone.
            .overlay(alignment: .bottomTrailing) { newListButton }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    StatusDot(waiting: queued, offline: offline, onDeviceOnly: onDeviceOnly)
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
                        isOwner: isOwner,
                        joinAList: { joining = true }
                    )
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
                        await makeList(named: name)
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
            .task {
                // Cheap, and the only way the dot stays honest while somebody is
                // looking at it: the list screen it belongs to drains the queue, and
                // so does every items screen.
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
    /// Makes a list, wherever it can.
    ///
    /// The server first, because a list made online should arrive with an id and no
    /// queue behind it. A transport failure is not an error here and never shows one:
    /// no signal and no server are the same state, and writing the list down locally
    /// is what the person asked for either way. It is queued, and the queue is what
    /// carries it to a server if one ever appears.
    ///
    /// This is S1 — the app is useful before it has anywhere to send anything.
    private func makeList(named name: String) async {
        do {
            _ = try await api.createList(named: name)
            await load()
        } catch APIError.transport {
            let made = cache.makeListHere(named: name, ownedBy: mine)
            cache.outbox.makeList(made)
            queued = cache.outbox.waiting
            lists = cache.lists()
            offline = true
        } catch {
            self.error = (error as? APIError)?.errorDescription ?? error.localizedDescription
        }
    }

    /// This person's id, for a list made with nobody to ask.
    ///
    /// Zero where there is no server and so no account. It is only ever compared with
    /// itself on this device — the server decides ownership from who sent the
    /// operation, not from what the device claimed.
    private var mine: Int64 { 0 }

    private func sendQueued() async {
        guard !draining, cache.outbox.waiting > 0 else { return }
        draining = true
        let drained = await cache.outbox.drain(through: api)
        draining = false
        queued = cache.outbox.waiting

        // Lists made here have just been given the server's own ids. Done before the
        // reload below, so the screen never shows the same list twice — once under
        // this device's numbering and once under the server's.
        for adopted in drained.adopted {
            if let local = cache.lists().first(where: { $0.uuid == adopted.uuid }) {
                cache.adopt(local, as: adopted.real)
            }
        }

        if !drained.adopted.isEmpty {
            lists = cache.lists()
        }
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
            // Asked once the lists have arrived rather than beside them, because
            // nothing on this screen waits for it — a menu item appearing a moment
            // late is better than a screen that waits for a question about
            // administration before it shows anybody their shopping.
            isOwner = (try? await api.whoAmI().isOwner) ?? isOwner

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
