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
    @State private var units: [Unit] = []
    @State private var line = ""
    @State private var editing: Item?
    @State private var confirmingClear = false
    @State private var error: String?
    @State private var loaded = false
    @FocusState private var typing: Bool

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    private var done: [Item] { items.filter(\.isDone) }

    /// Rows print a unit, the editor picks one. Built here rather than fetched twice.
    private var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: units.map { ($0.id, $0.name) })
    }

    var body: some View {
        SwiftUI.List {
            Section {
                HStack {
                    TextField("Add an item", text: $line)
                        .focused($typing)
                        .submitLabel(.done)
                        .onSubmit { Task { await add() } }
                        .autocorrectionDisabled()
                    if !line.isEmpty {
                        Button("Add") { Task { await add() } }
                    }
                }
            }

            if outstanding.isEmpty && loaded {
                Section {
                    Text(items.isEmpty ? "Nothing on this list yet." : "All done.")
                        .foregroundStyle(.secondary)
                }
            }

            Section {
                ForEach(outstanding) { item in
                    row(item)
                }
            }

            // What is already in the trolley, out of the way of what is not.
            if !done.isEmpty {
                Section {
                    ForEach(done) { item in
                        row(item)
                    }
                } header: {
                    HStack {
                        Text("\(done.count) done")
                        Spacer()
                        Button("Clear", role: .destructive) { confirmingClear = true }
                            .textCase(nil)
                    }
                }
            }
        }
        .navigationTitle(list.name)
        .navigationBarTitleDisplayMode(.inline)
        .refreshable { await load() }
        .task { await load() }
        .task { await watch() }
        // Coming back from the background is the one gap the stream cannot cover:
        // iOS tears the connection down and the reconnect has not happened yet.
        .onChange(of: phase) { _, now in
            if now == .active { Task { await load() } }
        }
        .sheet(item: $editing) { item in
            ItemEditor(item: item, units: units) { name, amount, unitID in
                await attempt {
                    try await api.update(item, on: list, name: name, amount: amount, unitID: unitID)
                }
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
        .swipeActions(edge: .trailing) {
            // Delete first, so it is what a full swipe commits to: that was the whole
            // gesture before edit existed, and changing what it does silently is how
            // you delete something you meant to correct.
            Button(role: .destructive) {
                Task { await remove(item) }
            } label: {
                Label("Delete", systemImage: "trash")
            }

            Button {
                editing = item
            } label: {
                Label("Edit", systemImage: "pencil")
            }
            .tint(.accentColor)
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

        await attempt { try await api.add(typed, to: list) }
    }

    private func toggle(_ item: Item) async {
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

    private func load() async {
        do {
            async let items = api.items(on: list)
            async let units = api.units()
            (self.items, self.units) = try await (items, units)
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
