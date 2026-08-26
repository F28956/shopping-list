import SwiftUI

/// What is on one list, on a machine with a keyboard.
///
/// The same grouping and the same rules as the phone; the differences are the ones a
/// desktop actually changes. Swipes become a context menu and a hover control, the
/// add field keeps focus so a shop can be typed in one go, and rows stay compact
/// because there is no thumb to aim.
struct MacItemsView: View {
    let api: API
    let list: List
    @Environment(Identity.self) private var identity

    @State private var items: [Item] = []
    @State private var units: [Unit] = []
    @State private var tags: [Tag] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var line = ""
    @State private var offered: [String] = []
    @State private var asking: Task<Void, Never>?
    @State private var editing: Editing?
    @State private var confirmingClear = false
    @State private var error: String?
    @FocusState private var typing: Bool

    struct Editing: Identifiable {
        let item: Item
        let attached: [Tag]
        var id: Int64 { item.id }
    }

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    private var done: [Item] { items.filter(\.isDone) }
    private var categories: [ItemGroup] { grouped(outstanding, by: tags) }
    private var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: units.map { ($0.id, $0.name) })
    }

    var body: some View {
        SwiftUI.List {
            if truncated {
                Text("Showing \(items.count) of \(Int(total)). This list is long enough to be worth splitting.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            if outstanding.isEmpty {
                Text(items.isEmpty ? "Nothing on this list yet." : "All done.")
                    .foregroundStyle(.secondary)
            }

            ForEach(categories) { category in
                Section {
                    ForEach(category.items) { row($0) }
                } header: {
                    if categories.count > 1 { Text(category.heading) }
                }
            }

            if !done.isEmpty {
                Section {
                    ForEach(done) { row($0) }
                } header: {
                    HStack {
                        Text("\(done.count) done")
                        Spacer()
                        if list.mayEdit {
                            Button("Clear") { confirmingClear = true }
                                .buttonStyle(.link)
                        }
                    }
                }
            }
        }
        .safeAreaInset(edge: .bottom) { addBar }
        .navigationTitle(list.name)
        .task { await load() }
        .task { await watch() }
        .sheet(item: $editing) { target in
            MacItemEditor(
                item: target.item,
                units: units,
                tags: tags,
                attached: target.attached
            ) { edit in
                await attempt { try await apply(edit, to: target) }
            }
        }
        .confirmationDialog(
            "Clear \(done.count) done \(done.count == 1 ? "item" : "items")?",
            isPresented: $confirmingClear
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

    /// The add field, pinned under the list.
    ///
    /// At the bottom rather than the top: it is where the cursor stays while a shop
    /// is typed, and a field that pushes the list down every time a suggestion
    /// appears is a field you stop trusting.
    @ViewBuilder
    private var addBar: some View {
        if list.mayEdit {
            VStack(spacing: 0) {
                if typing && !offered.isEmpty {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(offered, id: \.self) { suggestion in
                            Button {
                                line = suggestion
                                typing = true
                            } label: {
                                HStack {
                                    Image(systemName: "clock.arrow.circlepath")
                                        .foregroundStyle(.secondary)
                                    Text(suggestion)
                                    Spacer()
                                }
                                .contentShape(Rectangle())
                                .padding(.vertical, 3)
                                .padding(.horizontal, 12)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.vertical, 4)
                    Divider()
                }

                HStack {
                    TextField("Add an item — try 2 kg apples", text: $line)
                        .textFieldStyle(.plain)
                        .focused($typing)
                        .onSubmit { Task { await add() } }
                    Button("Add") { Task { await add() } }
                        .disabled(line.trimmingCharacters(in: .whitespaces).isEmpty)
                        .keyboardShortcut(.defaultAction)
                }
                .padding(10)
            }
            .background(.bar)
        }
    }

    private func row(_ item: Item) -> some View {
        HStack {
            Text(item.name)
                .strikethrough(item.isDone)
                .foregroundStyle(item.isDone ? .secondary : .primary)
            Spacer()
            if let measure = item.measure(units: unitNames) {
                Text(measure)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
        .contentShape(Rectangle())
        // One click ticks it off, which is the thing being done most.
        .onTapGesture { Task { await toggle(item) } }
        .contextMenu {
            if list.mayEdit {
                Button(item.isDone ? "Put back" : "Cross off") {
                    Task { await toggle(item) }
                }
                Button("Edit…") { Task { await beginEditing(item) } }
                Divider()
                Button("Delete", role: .destructive) { Task { await remove(item) } }
            }
        }
    }

    // MARK: - Doing things

    private func add() async {
        let typed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !typed.isEmpty else { return }
        line = ""
        typing = true
        await attempt { try await api.add(typed, to: list) }
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

    private func beginEditing(_ item: Item) async {
        do {
            editing = Editing(item: item, attached: try await api.tags(on: item, in: list))
        } catch {
            self.error = error.localizedDescription
        }
    }

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

    /// Keeps the window in step with the phone, the watch and the browser.
    private func watch() async {
        var reconnecting = false

        while !Task.isCancelled {
            if reconnecting { await load() }

            do {
                for try await _ in try await api.changes(on: list) {
                    await load()
                }
            } catch let problem as APIError {
                if case .unauthorized = problem {
                    identity.signOut()
                    return
                }
            } catch {}

            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
        }
    }

    private func load() async {
        do {
            async let items = api.items(on: list)
            async let units = api.units()
            async let tags = api.tags()
            let (listing, loadedUnits, loadedTags) = try await (items, units, tags)
            self.items = listing.items
            self.total = listing.total
            self.truncated = listing.truncated
            self.units = loadedUnits
            self.tags = loadedTags
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
    }
}
