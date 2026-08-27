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
    /// Asked once, and worth asking: changing servers throws away everything on this
    /// device (C4).
    @State private var changingServer = false
    /// Whether this person administers this server, which decides whether the screen
    /// that manages it exists. Hiding it is a courtesy: every route behind it is
    /// refused in the service layer to anybody else.
    /// There is no server, because somebody said so on the first screen. Read once:
    /// it only changes by leaving this screen entirely.
    private let onDeviceOnly = ServerDirectory.isOnDeviceOnly
    @State private var isOwner = false
    @State private var managingServer = false

    /// Forgets the server and everything that came from it.
    ///
    /// The order matters only in that the address goes last: if anything above throws,
    /// the device is still pointed at a server it can be signed into again, rather than
    /// at nothing with a cache full of somebody else's ids.
    private func leaveThisServer() {
        // `forgetEverything` takes the outbox with it — see `Cache`.
        cache.forgetEverything()
        identity.signOut()
        ServerDirectory.forget()
    }

    /// What an empty screen says.
    ///
    /// Three different emptinesses and they are not the same news. On a device kept to
    /// itself there is nothing wrong at all — nobody has written a list yet — and
    /// saying "can't reach the server" there would be reporting a failure that did not
    /// happen and could not.
    private var emptyTitle: String {
        if onDeviceOnly { return "No lists yet" }
        return offline ? "Can't reach the server" : "Couldn't load your lists"
    }

    /// The menu behind the plus.
    ///
    /// Its own property rather than inline: the toolbar closure grew past what the
    /// Swift type checker will do in reasonable time, and the error it gives for that
    /// names the whole expression rather than the part at fault.
    @ViewBuilder
    private var menuItems: some View {
        Button("New list", systemImage: "plus") { naming = .create }
        Button("Join a list", systemImage: "person.badge.plus") { joining = true }

        Divider()

        if isOwner {
            Button("Who may sign in", systemImage: "person.2.badge.key") {
                managingServer = true
            }
            .accessibilityIdentifier("manage-server")
        }

        Button(
            onDeviceOnly ? "Use a server" : "Change server",
            systemImage: "server.rack",
            role: onDeviceOnly ? nil : .destructive
        ) {
            changingServer = true
        }
        .accessibilityIdentifier("change-server")
    }

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
                            emptyTitle,
                            systemImage: onDeviceOnly
                                ? "checklist"
                                : (offline ? "icloud.slash" : "exclamationmark.triangle")
                        )
                    } description: {
                        Text(
                            onDeviceOnly
                                ? "Make one with the button above. It stays on this phone."
                                : (offline
                                    ? "Your lists will appear as soon as there is a connection."
                                    : "Whether you have any is not known yet.")
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
                    Menu { menuItems } label: { Label("Add", systemImage: "plus") }
                        .accessibilityIdentifier("list.new")
                }
            }
            // C4. Not a precaution: the cache holds rows keyed by ids and uuids the
            // old server minted, and the history and suggestions belong to an account
            // on it. Carrying them across would show one server's lists under another
            // server's name.
            .alert("Change server?", isPresented: $changingServer) {
                Button("Cancel", role: .cancel) {}
                Button("Change server", role: .destructive) { leaveThisServer() }
            } message: {
                Text(
                    """
                    This signs you out and removes everything stored on this device. \
                    Anything still waiting to be sent will be lost.
                    """
                )
            }
            .sheet(isPresented: $managingServer) {
                ServerPeopleView(api: api)
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
