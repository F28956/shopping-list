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
    /// There is no server. The default -- see `ServerDirectory`.
    @State private var onDeviceOnly = ServerDirectory.isOnDeviceOnly
    @State private var fresh = false
    @State private var loaded = false
    /// How many changes made here are still waiting to be sent.
    @State private var waiting = 0
    /// The rows carrying one of them — see the phone's `ItemsView`.
    @State private var unsent: Set<String> = []
    /// See the phone's `ItemsView.refused`.
    @State private var refused = false
    /// Guards against a drain and a reload calling each other round in a circle.
    @State private var draining = false
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

            // Not on a Mac with no server: everything is queued there and nothing
            // ever leaves, so a permanent "2 changes waiting to be sent" would be
            // reporting the arrangement rather than a problem.
            if (offline || waiting > 0 || refused) && !onDeviceOnly {
                OfflineNote(offline: offline, waiting: waiting, refused: refused)
            }

            // "Nothing on this list yet" is a claim, and after a load that failed
            // with nothing cached it is a claim nobody has checked.
            if items.isEmpty && loaded && !fresh && !onDeviceOnly {
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

            // Last, and on the detail's own toolbar, which is what puts it at the far
            // right of the window: a split view renders the sidebar's items over the
            // sidebar and the detail's over the detail, so a dot declared on the
            // sidebar sits in the middle of the title bar rather than the end of it.
            //
            // One dot for the window, not one per pane — the two halves' toolbars merge
            // into a single title bar, and there is one connection and one queue behind
            // them either way.
            // No pill behind it: macOS 26 gives every toolbar item a control's
            // background, which turns a thing you read into a thing that looks like it
            // wants pressing. Asked for where it exists, and simply not asked for
            // where it does not -- on 14 and 15 a toolbar item has no background to
            // hide, so the dot already sits bare.
            // Absent entirely with no server -- there is no question for a dot to
            // answer, see `StatusDot`. The whole item goes rather than its contents,
            // for the same reason the background is hidden below: an empty item is
            // still a shape.
            if !onDeviceOnly {
                if #available(macOS 26.0, *) {
                    ToolbarItem(placement: .primaryAction) {
                        StatusDot(waiting: waiting, offline: offline)
                    }
                    .sharedBackgroundVisibility(.hidden)
                } else {
                    ToolbarItem(placement: .primaryAction) {
                        StatusDot(waiting: waiting, offline: offline)
                    }
                }
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
        // Settings changes this under our feet, and storage is not observable state.
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            onDeviceOnly = ServerDirectory.isOnDeviceOnly
        }
        .task { await loadReference() }
        .task {
            showWhatWeHave()
            refreshUnsent()
            await load()
        }
        .task { await keepTrying() }
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
                                // Added outright, as on the phone: picking something
                                // you have bought before is already the whole
                                // decision, and a second press to confirm it asks
                                // nothing. It goes through the same resolve as
                                // anything typed, so it arrives measured and filed
                                // the way it was last time.
                                line = suggestion
                                typing = true
                                // What was accepted is no longer a suggestion.
                                suggestions.clear()
                                Task { await add() }
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
                        .fixedSize()
                        // The name never gives way. A window narrow enough to squeeze
                        // a row should lose the labels on the categories, not the word
                        // that says what to buy.
                        .layoutPriority(1)

                    // Where it lives, on the row itself. The list is ordered by the
                    // same tags, so these read as a label on a sorted list rather
                    // than as a second organising scheme — and they give way in two
                    // steps as the window narrows; see `MacTagStrip`.
                    MacTagStrip(tags: item.tagIDs.compactMap { tagsByID[$0] })

                    // Quietly, on the row. A change that has not been sent is a detail
                    // about that line, not news about the app — and a laptop on a train
                    // would have every line marked, which is a banner by another name.
                    if unsent.contains(item.uuid) {
                        Image(systemName: "clock")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Waiting to be sent")
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
        // See the phone's `ItemsView.add` for the negative id and why it never leaves.
        //
        // **What the line does is not decided here.** Which unit a bare name lands in,
        // whether `Milk` is the `milk` already on the list, whether a crossed-off row
        // comes back -- all of it is `parsing::add`, compiled in and shared with the
        // server. This screen used to make its own row every time, so typing the same
        // thing twice made two of them and nothing was going to merge them.
        // The whole memory. Picking one entry here meant picking it by the typed
        // line, which found nothing for anything carrying a quantity -- see
        // `QuickAdd.resolve`.
        let decision = QuickAdd.resolve(
            typed,
            units: units,
            rows: items,
            history: cache.history(on: list)
        )

        switch decision {
        case .existing(let uuid, let putBack):
            guard let alike = items.first(where: { $0.uuid == uuid }) else { return }
            // A fresh uuid, not the row's: handing the server its own would take the
            // early return in `create` and skip the putting-back.
            // Its own name and amount, not the typed line: this is the row the
            // shared rule chose, and saying so leaves the server nothing to choose.
            cache.outbox.add(
                uuid: UUID().uuidString.lowercased(),
                localID: alike.id,
                name: alike.name,
                amount: alike.amount,
                unitID: alike.unitID,
                on: list
            )
            cache.remember(alike, on: list, isNew: true)
            if putBack {
                show { rows in rows.map { $0.uuid == uuid ? $0.withDone(false) : $0 } }
            }
            await drain()
            return

        case .new(let row):
            let uuid = UUID().uuidString.lowercased()
            let local = Item(
                id: -Int64(Date().timeIntervalSince1970 * 1000),
                uuid: uuid,
                name: row.name,
                amount: row.amount,
                unitID: row.unitID,
                doneAt: nil,
                tagIDs: row.tagIDs
            )
            cache.remember(local, on: list, isNew: true)
            cache.outbox.add(
                uuid: uuid,
                localID: local.id,
                name: local.name,
                amount: local.amount,
                unitID: local.unitID,
                on: list
            )
            // Where the history said it belongs, said out loud. The add itself has no
            // field for tags, and the server's own filing step only runs when it is
            // given a line -- so this is how what was drawn here becomes what is
            // stored there. Behind the add in an ordered queue, so the row exists.
            for tagID in local.tagIDs {
                cache.outbox.tag(local, on: list, tagID: tagID, attached: true)
            }
            show { $0 + [local] }
        }

        await drain()
    }

    /// Crosses something off, or puts it back, whether or not there is a connection —
    /// see the phone's `ItemsView.toggle` for why the screen changes first.
    private func toggle(_ item: Item) async {
        guard list.mayEdit else { return }

        let done = !item.isDone
        cache.outbox.setDone(item, on: list, done: done)
        show { rows in rows.map { $0.uuid == item.uuid ? $0.withDone(done) : $0 } }
        await drain()
    }

    /// Sends what is queued, then says what became of it — see the phone's copy.
    private func drain() async {
        guard !draining else { return }

        // Read the queue back even when there is nothing to send, and *before* the
        // early return. The lists screen drains the same queue on its own — it has to,
        // because the app opens there — so this screen's count can go stale the moment
        // that happens. Returning early without refreshing left "3 changes waiting to
        // be sent" on a screen whose queue had been empty for minutes.
        refreshUnsent()
        guard cache.outbox.waiting > 0 else { return }

        draining = true
        let drained = await cache.outbox.drain(through: api)
        draining = false

        refreshUnsent()
        refused = drained.refused
        if let lost = drained.lost.first { error = lost }
        if drained.sent > 0 { await load() }
    }

    private func refreshUnsent() {
        let queued = cache.outbox.forList(list)
        unsent = Set(queued.map(\.itemUUID))
        waiting = queued.count
    }

    /// Tries the queue again while anything is in it.
    ///
    /// A load drains on success, and a load happens when the change stream reconnects.
    /// That is the right moment when there is a stream to reconnect, and the wrong thing
    /// to depend on entirely — a laptop closed on a train and opened in a station should
    /// send what it is holding without waiting for somebody else to touch the list.
    private func keepTrying() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(10))
            await drain()
        }
    }

    /// Rewrites what is on screen, and remembers it — see the phone's copy.
    private func show(_ change: ([Item]) -> [Item]) {
        items = change(items)
        cache.remember(items: items, on: list)
        refreshUnsent()
    }

    /// The server's answer with this device's unsent changes laid back over it.
    private func withUnsent(_ fromServer: [Item]) -> [Item] {
        let queued = cache.outbox.forList(list)
        guard !queued.isEmpty else { return fromServer }

        // Only rows this device *created* and has not sent are carried across. Any
        // queued operation used to qualify, which meant a tick queued against a row
        // somebody else had deleted put that row back on screen as a ghost — present
        // here, gone everywhere else, and impossible to get rid of.
        let known = Set(fromServer.map(\.uuid))
        let made = Set(
            queued.filter { $0.kind == QueuedOperation.Kind.add }.map(\.itemUUID)
        )
        var rows = fromServer + items.filter { !known.contains($0.uuid) && made.contains($0.uuid) }

        for operation in queued {
            switch operation.kind {
            case QueuedOperation.Kind.setDone:
                rows = rows.map { $0.uuid == operation.itemUUID ? $0.withDone(operation.done) : $0 }
            case QueuedOperation.Kind.delete:
                rows = rows.filter { $0.uuid != operation.itemUUID }
            case QueuedOperation.Kind.update:
                rows = rows.map { row in
                    guard row.uuid == operation.itemUUID else { return row }
                    return Item(
                        id: row.id,
                        uuid: row.uuid,
                        name: operation.editedName ?? row.name,
                        amount: operation.editedAmount ?? row.amount,
                        unitID: operation.editedUnitID ?? row.unitID,
                        doneAt: row.doneAt,
                        tagIDs: row.tagIDs
                    )
                }
            case QueuedOperation.Kind.clearDone:
                rows = rows.filter { !operation.sweptUUIDs.contains($0.uuid) }
            default:
                break
            }
        }
        return rows
    }

    private func remove(_ item: Item) async {
        guard list.mayEdit else { return }
        cache.outbox.delete(item, on: list)
        show { rows in rows.filter { $0.uuid != item.uuid } }
        await drain()
    }

    private func clearDone() async {
        guard list.mayEdit, !done.isEmpty else { return }
        let swept = done
        cache.outbox.clearDone(swept, on: list)
        show { rows in rows.filter { row in !swept.contains { $0.uuid == row.uuid } } }
        await drain()
    }

    private func beginEditing(_ item: Item) async {
        do {
            editing = Editing(item: item, attached: try await api.tags(on: item, in: list))
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func apply(_ edit: ItemEdit, to target: Editing) async throws {
        cache.outbox.update(
            target.item,
            on: list,
            name: edit.name,
            amount: edit.amount,
            unitID: edit.unitID
        )
        show { rows in
            rows.map {
                guard $0.uuid == target.item.uuid else { return $0 }
                return Item(
                    id: $0.id,
                    uuid: $0.uuid,
                    name: edit.name,
                    amount: edit.amount,
                    unitID: edit.unitID,
                    doneAt: $0.doneAt,
                    tagIDs: $0.tagIDs
                )
            }
        }
        await drain()

        // Tags are still online-only, and say so by failing rather than by pretending.

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
        } catch {
            seedReference()
        }
    }

    /// Falls back to the reference set that shipped with the app.
    ///
    /// Written to the cache as well as used, so the next screen finds it without asking
    /// — and so that a Mac which later gains a server simply overwrites it with that
    /// server's answer, ids and all. The ids are the same ids; see `Reference`.
    private func seedReference() {
        // The cache first, and only then the bundle -- see the phone's copy for the
        // bug this shape exists to prevent: seeding over a stored order silently
        // undoes the aisle order somebody arranged.
        if units.isEmpty {
            let remembered = cache.units()
            units = remembered.isEmpty ? Reference.units : remembered
            if remembered.isEmpty { cache.remember(units: units) }
        }
        if tags.isEmpty {
            let remembered = cache.tags(on: list)
            tags = remembered.isEmpty ? Reference.tags : remembered
            if remembered.isEmpty { cache.remember(tags: tags, on: list) }
        }
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
            self.items = withUnsent(listing.items)
            self.total = listing.total
            self.truncated = listing.truncated
            error = nil
            offline = false
            fresh = true
            loaded = true
            // The server is reachable, so anything waiting can go now.
            await drain()
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
