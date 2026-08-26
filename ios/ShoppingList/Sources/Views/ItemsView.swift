import SwiftUI

/// What is on one list: the screen this app exists for.
///
/// Adding, ticking off, correcting and clearing. Tags and sharing are deliberately
/// absent — a phone in a shop is for the handful of things you actually do standing
/// in one, and every control that is not one of those is in the way.
struct ItemsView: View {
    let api: API
    let list: List
    @Environment(Identity.self) private var identity
    @Environment(\.scenePhase) private var phase

    @State private var items: [Item] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var units: [Unit] = []
    @State private var suggestions = Suggestions()
    @State private var line = ""
    @State private var tags: [Tag] = []
    @State private var editing: Editing?
    @State private var confirmingClear = false
    @State private var ordering = false
    @State private var error: String?
    @State private var loaded = false
    @FocusState private var typing: Bool

    /// An item and what it is already filed under, fetched before the sheet opens so
    /// the editor never renders a tag section it is about to change under your thumb.
    struct Editing: Identifiable {
        let item: Item
        let attached: [Tag]
        var id: Int64 { item.id }
    }

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    /// Outstanding items under their category heading, in the order the shop is laid
    /// out. The rule is shared with the watch and matches the browser's — see
    /// `grouped(_:by:)`.
    private var categories: [ItemGroup] { grouped(outstanding, by: tags) }
    private var done: [Item] { items.filter(\.isDone) }

    /// Rows print a unit, the editor picks one. Built here rather than fetched twice.
    private var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: units.map { ($0.id, $0.name) })
    }

    var body: some View {
        SwiftUI.List {
            if list.mayEdit {
            Section {
                HStack {
                    TextField("Add an item — try 2 kg apples", text: $line)
                        .focused($typing)
                        .submitLabel(.done)
                        .onSubmit { Task { await add() } }
                        .autocorrectionDisabled()
                    if !line.isEmpty {
                        Button("Add") { Task { await add() } }
                    }
                }
            }
            }

            // Only while the field has focus: a permanent list of things you might
            // want is clutter on a screen whose job is what you actually need.
            if typing && !suggestions.offered.isEmpty {
                Section { suggestionSection }
            }

            if truncated {
                Section { truncationNotice }
            }

            if outstanding.isEmpty && loaded {
                Section {
                    Text(items.isEmpty ? "Nothing on this list yet." : "All done.")
                        .foregroundStyle(.secondary)
                }
            }

            ForEach(categories) { category in
                Section {
                    ForEach(category.items) { item in
                        row(item)
                    }
                } header: {
                    // Only worth a heading when there is more than one: a single
                    // "Other" above every item on the list says nothing.
                    if categories.count > 1 {
                        Text(category.heading)
                    }
                }
            }

            // What is already in the trolley, out of the way of what is not.
            if !done.isEmpty {
                Section {
                    ForEach(done) { item in
                        row(item)
                    }
                } header: {
                    doneHeader
                }
            }
        }
        .onChange(of: line) { _, typed in
            suggestions.update(typed: typed) { wanted in
                try await api.suggestions(matching: wanted, on: list)
            }
        }
        .navigationTitle(list.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    ordering = true
                } label: {
                    Label("Tag order", systemImage: "arrow.up.arrow.down")
                }
                .accessibilityIdentifier("order.open")
            }
        }
        .sheet(isPresented: $ordering) {
            TagOrderSheet(list: list, tags: tags) { chosen in
                await attempt { try await api.setTagOrder(chosen, on: list) }
                await loadReference()
            }
        }
        .refreshable { await load() }
        .task { await loadReference() }
        .task { await load() }
        .task { await watch() }
        // Coming back from the background is the one gap the stream cannot cover:
        // iOS tears the connection down and the reconnect has not happened yet.
        .onChange(of: phase) { _, now in
            if now == .active { Task { await load() } }
        }
        .sheet(item: $editing) { target in
            ItemEditor(
                item: target.item,
                units: units,
                tags: tags,
                attached: target.attached
            ) { edit in
                await attempt { try await apply(edit, to: target) }
            }
        }
        // Asked rather than assumed: this is the one control on the screen that takes
        // several rows at once, and a mis-tap cannot be undone from here.
        .confirmationDialog(
            "Clear \(done.count) done \(done.count == 1 ? "item" : "items")?",
            isPresented: $confirmingClear,
            titleVisibility: .visible
        ) {
            Button("Clear", role: .destructive) { Task { await clearDone() } }
            Button("Cancel", role: .cancel) {}
        }
        .alert("Something went wrong", isPresented: .constant(error != nil)) {
            Button("OK") { error = nil }
        } message: {
            Text(error ?? "")
        }
    }

    /// The done section's heading, with the one control that empties it.
    ///
    /// Its own property for the same reason as `truncationNotice`: `body` is long
    /// enough that the type-checker gives up on it, and a `Button` whose title is
    /// conditional is exactly the kind of thing it gives up on.
    private var doneHeader: some View {
        HStack {
            Text("\(done.count) done")
            Spacer()
            if list.mayEdit {
                Button("Clear", role: .destructive) { confirmingClear = true }
                    .textCase(nil)
            }
        }
    }

    /// The things this list has bought before that match what is being typed.
    private var suggestionSection: some View {
        ForEach(suggestions.offered, id: \.self) { suggestion in
            Button {
                // Fills the field rather than adding outright. What is typed may
                // carry a quantity -- "2 kg app" -- and the only thing that knows
                // what a line means is the server, so guessing here is how the phone
                // and the browser start disagreeing about it.
                line = suggestion
            } label: {
                HStack {
                    Image(systemName: "clock.arrow.circlepath")
                        .foregroundStyle(.secondary)
                        .font(.footnote)
                    Text(suggestion)
                    Spacer()
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }

    /// Said rather than hidden: a prefix shown as the whole list makes the rows that
    /// did not fit look deleted rather than merely elsewhere. The browser has always
    /// said this; these apps decoded the flag and never read it.
    ///
    /// Its own property because `body` is at the limit of what the type-checker will
    /// infer in one expression, and an interpolated string inside a view builder is
    /// an expensive thing to put there.
    private var truncationNotice: some View {
        let shown = items.count
        let all = Int(total)

        return Text("Showing \(shown) of \(all). This list is long enough to be worth splitting.")
            .font(.footnote)
            .foregroundStyle(.secondary)
    }

    private func row(_ item: Item) -> some View {
        Button {
            Task { await toggle(item) }
        } label: {
            HStack {
                Text(item.name)
                    .strikethrough(item.isDone)
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                Spacer()
                if let measure = item.measure(units: unitNames) {
                    Text(measure)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        // A viewer is given a list to read, not one covered in controls that would
        // refuse them — the same rule the browser follows.
        .swipeActions(edge: .trailing) {
            if list.mayEdit {
            // Delete first, so it is what a full swipe commits to: that was the whole
            // gesture before edit existed, and changing what it does silently is how
            // you delete something you meant to correct.
            Button(role: .destructive) {
                Task { await remove(item) }
            } label: {
                Label("Delete", systemImage: "trash")
            }

            Button {
                Task { await beginEditing(item) }
            } label: {
                Label("Edit", systemImage: "pencil")
            }
            .tint(.accentColor)
            }
        }
    }

    // MARK: - Doing things

    private func add() async {
        let typed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !typed.isEmpty else { return }

        // Cleared before the request rather than after, so the next item can be typed
        // straight away — the same reason the web form sits outside the swap.
        line = ""
        typing = true
        suggestions.clear()

        await attempt { try await api.add(typed, to: list) }
    }

    /// Opens the editor, once we know what the item is already filed under.
    private func beginEditing(_ item: Item) async {
        do {
            editing = Editing(item: item, attached: try await api.tags(on: item, in: list))
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

    /// Saves an edit: the fields, then the tags that changed.
    ///
    /// Tags have their own routes rather than being part of the update, so this is
    /// the diff. Only what changed is sent -- re-attaching a tag an item already has
    /// would be a conflict, and detaching one it never had a miss.
    private func apply(_ edit: ItemEdit, to target: Editing) async throws {
        try await api.update(
            target.item,
            on: list,
            name: edit.name,
            amount: edit.amount,
            unitID: edit.unitID
        )

        let before = Set(target.attached.map(\.id))
        for tag in tags where edit.tagIDs.contains(tag.id) && !before.contains(tag.id) {
            try await api.attach(tag, to: target.item, on: list)
        }
        for tag in target.attached where !edit.tagIDs.contains(tag.id) {
            try await api.detach(tag, from: target.item, on: list)
        }
    }

    private func toggle(_ item: Item) async {
        guard list.mayEdit else { return }
        await attempt { try await api.setDone(item, on: list, done: !item.isDone) }
    }

    private func remove(_ item: Item) async {
        await attempt { try await api.delete(item, on: list) }
    }

    private func clearDone() async {
        await attempt { try await api.clearDone(on: list) }
    }

    /// Keeps this screen in step with the same list open somewhere else.
    ///
    /// Reconnects for as long as the screen is up, because a stream that ends is
    /// indistinguishable, from here, from a list where nothing is happening -- and
    /// silently showing a stale list is exactly what this is for. Each reconnect
    /// re-reads: whatever changed while the connection was down was never sent.
    private func watch() async {
        var reconnecting = false

        while !Task.isCancelled {
            if reconnecting { await load() }

            do {
                for try await _ in try await api.changes(on: list) {
                    await load()
                }
            } catch let problem as APIError {
                // A stream refused for want of a token is not a network hiccup, and
                // retrying it forever would hammer the server while signed out.
                if case .unauthorized = problem {
                    identity.signOut()
                    return
                }
            } catch {}

            // Losing the connection is ordinary -- a tunnel, a lock screen, a server
            // restart -- so it is not shown. Waiting keeps a server that is refusing
            // everything from being asked as fast as the loop can go round.
            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
        }
    }

    /// Runs something that changes the list, then reloads.
    ///
    /// Reloading rather than patching the array in place: the server decides the order
    /// and what a line meant, and guessing at either is how a phone comes to disagree
    /// with the browser about what is on the list.
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

    /// Units and tags: fetched once, when the screen appears.
    ///
    /// They are seeded by migration and change when the server is deployed, not when
    /// somebody ticks something off — and `load()` runs on every change anyone makes
    /// to this list, from any device. Fetching them there meant thirty-one units and
    /// twenty-one tags crossing the network for each tick, to say what they said the
    /// time before.
    private func loadReference() async {
        do {
            async let units = api.units()
            async let tags = api.tags(orderedFor: list)
            (self.units, self.tags) = try await (units, tags)
        } catch {
            // Not shown: without these, rows lose their measure and their grouping,
            // which is a poorer list rather than no list. `load()` reports what
            // actually stops the screen working.
        }
    }

    private func load() async {
        do {
            let listing = try await api.items(on: list)
            self.items = listing.items
            self.total = listing.total
            self.truncated = listing.truncated
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
