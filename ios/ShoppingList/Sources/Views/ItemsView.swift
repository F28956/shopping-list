import SwiftUI

/// What is on one list: the screen this app exists for.
///
/// Adding, ticking off, and removing. Editing, tags and sharing are deliberately
/// absent — a phone in a shop is for the two things you actually do standing in one,
/// and every control that is not one of those is in the way.
struct ItemsView: View {
    let api: API
    let list: List
    @Environment(Identity.self) private var identity

    @State private var items: [Item] = []
    @State private var units: [Int64: String] = [:]
    @State private var line = ""
    @State private var error: String?
    @State private var loaded = false
    @FocusState private var typing: Bool

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    private var done: [Item] { items.filter(\.isDone) }

    var body: some View {
        SwiftUI.List {
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
                .onDelete { offsets in
                    Task { await remove(offsets.map { outstanding[$0] }) }
                }
            }

            // What is already in the trolley, out of the way of what is not.
            if !done.isEmpty {
                Section("\(done.count) done") {
                    ForEach(done) { item in
                        row(item)
                    }
                    .onDelete { offsets in
                        Task { await remove(offsets.map { done[$0] }) }
                    }
                }
            }
        }
        .navigationTitle(list.name)
        .navigationBarTitleDisplayMode(.inline)
        .refreshable { await load() }
        .task { await load() }
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
                Image(systemName: item.isDone ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                Text(item.name)
                    .strikethrough(item.isDone)
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                Spacer()
                if let measure = item.measure(units: units) {
                    Text(measure)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
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

    private func remove(_ items: [Item]) async {
        await attempt {
            for item in items {
                try await api.delete(item, on: list)
            }
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
