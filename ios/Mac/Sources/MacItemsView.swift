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

    private let cache = Cache.shared

    @State private var items: [Item] = []
    @State private var units: [Unit] = []
    @State private var tags: [Tag] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var line = ""
    @State private var suggestions = Suggestions()
    @State private var editing: Editing?
    @State private var confirmingClear = false
    @State private var ordering = false
    @State private var error: String?
    /// See `ListsView.offline` on the phone.
    @State private var offline = false
    @State private var fresh = false
    @State private var loaded = false
    @FocusState private var typing: Bool

    struct Editing: Identifiable {
        let item: Item
        let attached: [Tag]
        var id: Int64 { item.id }
    }

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    private var done: [Item] { items.filter(\.isDone) }
    /// Outstanding items in the order the shop is walked, with no headings.
    ///
    /// The categories decide the order and then get out of the way: what tells you
    /// where a thing lives is the tag on its own row, not a band across the list.
    private var ordered: [Item] { grouped(outstanding, by: tags).flatMap(\.items) }

    private var tagsByID: [Int64: Tag] {
        Dictionary(uniqueKeysWithValues: tags.map { ($0.id, $0) })
    }
    private var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: units.map { ($0.id, $0.name) })
    }

    var body: some View {
        SwiftUI.List {
            if truncated {
                Text("Showing \(items.count) of \(Int(total)). This list is long enough to be worth splitting.")
                    .accessibilityIdentifier("truncation.notice")
                    .accessibilityLabel(
                        "Showing \(items.count) of \(Int(total)). "
                            + "This list is long enough to be worth splitting."
                    )
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            if offline {
                OfflineNote()
            }

            // "Nothing on this list yet" is a claim, and after a load that failed
            // with nothing cached it is a claim nobody has checked.
            if items.isEmpty && loaded && !fresh {
                Text(
                    offline
                        ? "Can't reach the server. This list will appear as soon as there is a connection."
                        : "Couldn't load this list. What is on it is not known yet."
                )
                .foregroundStyle(.secondary)
            } else if outstanding.isEmpty {
                Text(items.isEmpty ? "Nothing on this list yet." : "All done.")
                    .foregroundStyle(.secondary)
            }

            ForEach(ordered) { row($0) }

            if !done.isEmpty {
                Section {
                    ForEach(done) { row($0) }
                } header: {
                    HStack {
                        Text("\(done.count) done")
                        Spacer()
                        if list.mayEdit {
                            Button("Clear") { confirmingClear = true }
                                .accessibilityIdentifier("clear.done")
                                .buttonStyle(.link)
                        }
                    }
                }
            }
        }
        .safeAreaInset(edge: .bottom) { addBar }
        .onChange(of: line) { _, typed in
            suggestions.update(typed: typed) { wanted in
                try await api.suggestions(matching: wanted, on: list)
            }
        }
        .navigationTitle(list.name)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    ordering = true
                } label: {
                    Label("Tag order", systemImage: "arrow.up.arrow.down")
                }
                .help("Which tag decides where an item sits")
                .accessibilityIdentifier("order.open")
            }
        }
        .sheet(isPresented: $ordering) {
            TagOrderSheet(
                list: list,
                tags: tags,
                // What the list's items actually carry, so the sheet can say which
                // of twenty-one names are the ones that would change anything.
                inUse: Set(items.flatMap(\.tagIDs))
            ) { chosen in
                await attempt { try await api.setTagOrder(chosen, on: list) }
                // The order changed, so what leads changed: read it back rather than
                // reordering the copy held here and hoping the two agree.
                await loadReference()
            }
        }
        .task { await loadReference() }
        .task {
            showWhatWeHave()
            await load()
        }
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
                if typing && !suggestions.offered.isEmpty {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(suggestions.offered, id: \.self) { suggestion in
                            Button {
                                line = suggestion
                                typing = true
                                // What was accepted is no longer a suggestion.
                                suggestions.clear()
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
                            .accessibilityIdentifier("suggestion.\(suggestion)")
                        }
                    }
                    .padding(.vertical, 4)
                    Divider()
                }

                HStack(spacing: 8) {
                    Image(systemName: "plus.circle.fill")
                        .foregroundStyle(.tint)
                        .imageScale(.large)

                    // Bordered, not plain. A plain field on a bar background is the
                    // background, and the one control the screen exists for should
                    // not have to be discovered.
                    TextField("Add an item — try 2 kg apples", text: $line)
                        .accessibilityIdentifier("add.field")
                        .textFieldStyle(.roundedBorder)
                        .controlSize(.large)
                        .focused($typing)
                        .onSubmit { Task { await add() } }

                    Button("Add") { Task { await add() } }
                        .accessibilityIdentifier("add.button")
                        .buttonStyle(.borderedProminent)
                        .disabled(line.trimmingCharacters(in: .whitespaces).isEmpty)
                        .keyboardShortcut(.defaultAction)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
            }
            .background(.bar)
            .overlay(alignment: .top) { Divider() }
        }
    }

    private func row(_ item: Item) -> some View {
        // The row opens the editor; the checkbox crosses off. That is the other way
        // round from the phone and the watch on purpose: those are held in a shop,
        // where crossing off is nearly all you do, and this is where the list gets
        // written. The phone has no checkbox for the same reason -- there, tapping
        // the row already means cross off, and a box would only repeat it.
        HStack(spacing: 8) {
            Toggle("", isOn: crossedOff(item))
                .toggleStyle(.checkbox)
                .labelsHidden()
                .disabled(!list.mayEdit)
                .accessibilityLabel(
                    item.isDone ? "Put \(item.name) back" : "Cross \(item.name) off"
                )
                .accessibilityIdentifier("cross.\(item.name)")

            Button {
                Task { await beginEditing(item) }
            } label: {
                HStack(spacing: 8) {
                    Text(item.name)
                        .strikethrough(item.isDone)
                        .foregroundStyle(item.isDone ? .secondary : .primary)

                    // Where it lives, on the row itself. The list is ordered by the
                    // same tags, so these read as a label on a sorted list rather
                    // than as a second organising scheme.
                    ForEach(item.tagIDs.compactMap { tagsByID[$0] }) { tag in
                        chip(tag)
                    }

                    Spacer(minLength: 8)

                    if let measure = item.measure(units: unitNames) {
                        Text(measure)
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!list.mayEdit)
            .accessibilityLabel(accessibleName(item))
            .accessibilityHint(list.mayEdit ? "Opens the editor" : "")
            .accessibilityIdentifier("item.\(item.name)")
        }
        .contextMenu {
            if list.mayEdit {
                Button("Edit…") { Task { await beginEditing(item) } }
                Button(item.isDone ? "Put back" : "Cross off") {
                    Task { await toggle(item) }
                }
                Divider()
                Button("Delete", role: .destructive) { Task { await remove(item) } }
            }
        }
    }

    /// The checkbox's state, and what ticking it means.
    ///
    /// The value comes from the item rather than from anything held here, so a change
    /// made on the phone moves this box too -- there is no second copy of "done" to
    /// fall out of step.
    private func crossedOff(_ item: Item) -> Binding<Bool> {
        Binding(
            get: { item.isDone },
            set: { _ in Task { await toggle(item) } }
        )
    }

    /// A tag beside an item: quiet, and not a control.
    ///
    /// Nothing here is tappable. Changing what an item is filed under is the editor's
    /// job, and a chip that sometimes removes a tag when you meant to cross the item
    /// off is the reason the phone keeps them in the sheet too.
    private func chip(_ tag: Tag) -> some View {
        Text(tag.emoji.flatMap { $0.isEmpty ? nil : "\($0) \(tag.name)" } ?? tag.name)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(.quaternary, in: Capsule())
            .accessibilityHidden(true)
            .accessibilityIdentifier("chip.\(tag.name)")
    }

    /// What the row says when it is read aloud rather than looked at.
    ///
    /// Struck-through text and grey are not information to a screen reader, and the
    /// measure sits in a separate label it would read as a loose number.
    private func accessibleName(_ item: Item) -> String {
        let measure = item.measure(units: unitNames).map { ", \($0)" } ?? ""
        let state = item.isDone ? ", crossed off" : ""
        // Spoken here rather than by the chips, which are hidden from VoiceOver: read
        // separately they arrive as loose words after the item with nothing to say
        // what they are.
        let filed = item.tagIDs.compactMap { tagsByID[$0]?.name }
        let under = filed.isEmpty ? "" : ", in \(filed.joined(separator: ", "))"
        return "\(item.name)\(measure)\(under)\(state)"
    }

    // MARK: - Doing things

    private func add() async {
        let typed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !typed.isEmpty else { return }
        line = ""
        typing = true
        suggestions.clear()
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

    /// Reference data, fetched once when the screen appears rather than on every
    /// reload — see the phone's copy for what that was costing.
    private func loadReference() async {
        do {
            async let units = api.units()
            async let tags = api.tags(orderedFor: list)
            let (fetchedUnits, fetchedTags) = try await (units, tags)
            cache.remember(units: fetchedUnits)
            cache.remember(tags: fetchedTags, on: list)
            (self.units, self.tags) = (fetchedUnits, fetchedTags)
        } catch {}
    }

    /// Puts the list up as it was last seen -- see the phone's copy.
    private func showWhatWeHave() {
        guard !fresh else { return }

        let rememberedUnits = cache.units()
        let rememberedTags = cache.tags(on: list)
        if !rememberedUnits.isEmpty { units = rememberedUnits }
        if !rememberedTags.isEmpty { tags = rememberedTags }

        let remembered = cache.items(on: list)
        guard !remembered.isEmpty else { return }
        items = remembered
        total = Int64(remembered.count)
        loaded = true
    }

    private func load() async {
        do {
            let listing = try await api.items(on: list)
            cache.remember(items: listing.items, on: list)
            self.items = listing.items
            self.total = listing.total
            self.truncated = listing.truncated
            error = nil
            offline = false
            fresh = true
        } catch let problem as APIError {
            if case .unauthorized = problem {
                identity.signOut()
            } else if case .transport = problem {
                // No signal is a state, not an event -- see the phone's ItemsView.
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
