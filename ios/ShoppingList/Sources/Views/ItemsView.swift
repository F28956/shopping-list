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

    private let cache = Cache.shared

    @State private var items: [Item] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var units: [Unit] = []
    @State private var suggestions = Suggestions()
    @State private var adding = false
    @State private var line = ""
    @State private var tags: [Tag] = []
    @State private var editing: Editing?
    @State private var confirmingClear = false
    @State private var ordering = false
    @State private var sharing = false
    @State private var error: String?
    @State private var loaded = false
    /// See `ListsView.offline`.
    @State private var offline = false
    @State private var fresh = false
    /// How many changes made here are still waiting to be sent.
    @State private var waiting = 0
    /// The rows carrying one of them. Marked on the row itself rather than with a
    /// banner: it is a detail about that line, not news about the app.
    @State private var unsent: Set<String> = []
    /// Something was refused and will not retry itself. The one state of the three in
    /// `docs/offline.md` that is worth interrupting somebody for.
    @State private var refused = false
    /// Guards against a drain and a reload calling each other round in a circle.
    @State private var draining = false

    /// An item and what it is already filed under, fetched before the sheet opens so
    /// the editor never renders a tag section it is about to change under your thumb.
    struct Editing: Identifiable {
        let item: Item
        let attached: [Tag]
        var id: Int64 { item.id }
    }

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    /// Outstanding items in the order this list is walked, with no headings.
    ///
    /// The tag that decides the order rides on the row instead. A heading says the
    /// same thing as the chip beneath it, and one of the two is redundant on a screen
    /// this narrow — see `row(_:)`.
    private var ordered: [Item] { grouped(outstanding, by: tags).flatMap(\.items) }
    private var done: [Item] { items.filter(\.isDone) }

    /// Rows print a unit, the editor picks one. Built here rather than fetched twice.
    private var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: units.map { ($0.id, $0.name) })
    }

    var body: some View {
        SwiftUI.List {
            // Nothing to say on a device kept to itself: nothing is stale, nothing is
            // waiting for a connection that is coming, and a line apologising for one
            // somebody declined is worse than silence.
            if (offline || waiting > 0 || refused) && !ServerDirectory.isOnDeviceOnly {
                Section { OfflineNote(offline: offline, waiting: waiting, refused: refused) }
            }

            if truncated {
                Section { truncationNotice }
            }

            // "Nothing on this list yet" is a claim, and with nothing cached and a
            // load that failed it is a claim nobody has checked. `fresh` is what earns
            // it, and only the server can set that.
            //
            // Except on a device kept to itself, where there is no server to have
            // checked with and this device is the only thing that could know. There,
            // empty means empty.
            if items.isEmpty && loaded && !fresh && !ServerDirectory.isOnDeviceOnly {
                Section {
                    Text(
                        offline
                            ? "Can't reach the server. This list will appear as soon as there is a connection."
                            : "Couldn't load this list. What is on it is not known yet."
                    )
                    .foregroundStyle(.secondary)
                }
            } else if outstanding.isEmpty && loaded {
                Section {
                    Text(items.isEmpty ? "Nothing on this list yet." : "All done.")
                        .foregroundStyle(.secondary)
                }
            }

            Section {
                ForEach(ordered) { item in
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
            // Beside the title rather than among the buttons. A toolbar item gets a
            // button's own background on iOS 26, which made a dot that does nothing
            // look like a control that does something — and the whole point of it is
            // to be read without being pressed.
            ToolbarItem(placement: .principal) {
                HStack(spacing: 6) {
                    Text(list.name)
                        .font(.headline)
                    StatusDot(
                        waiting: waiting,
                        offline: offline,
                        onDeviceOnly: ServerDirectory.isOnDeviceOnly
                    )
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    ordering = true
                } label: {
                    Label("Tag order", systemImage: "arrow.up.arrow.down")
                }
                .accessibilityIdentifier("order.open")
            }
            // Here as well as on the lists screen: this is where you are when you
            // think "somebody else should be able to see this", and a swipe on a row
            // two screens back is a control nobody finds.
            ToolbarItem(placement: .topBarTrailing) {
                // A share link names a server. With no server there is no link to
                // make, so the button is absent rather than present and failing.
                if !ServerDirectory.isOnDeviceOnly {
                    Button {
                        sharing = true
                    } label: {
                        Label("Share", systemImage: "person.badge.plus")
                    }
                    .accessibilityIdentifier("share.open")
                }
            }
        }
        // The one thing this screen is for, in the corner a thumb already is — the
        // same shape as the lists screen, so the two do not each have their own idea
        // of how adding works.
        .overlay(alignment: .bottomTrailing) {
            if list.mayEdit { addItemButton }
        }
        .sheet(isPresented: $adding) {
            AddItemSheet(line: $line, suggestions: suggestions) { await add() }
        }
        .sheet(isPresented: $sharing) {
            ShareSheet(list: list, api: api) {}
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
                await loadReference()
            }
        }
        .refreshable { await load() }
        .task { await loadReference() }
        .task {
            showWhatWeHave()
            refreshUnsent()
            // `load` drains on success, so what was queued in the shop yesterday goes
            // as soon as the first request gets through.
            await load()
        }
        .task { await watch() }
        .task { await keepTrying() }
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

    /// The one action this screen has.
    private var addItemButton: some View {
        Button {
            adding = true
        } label: {
            Image(systemName: "plus")
                .font(.title2.weight(.semibold))
                .frame(width: 56, height: 56)
        }
        .background(.tint, in: Circle())
        .foregroundStyle(.white)
        .shadow(radius: 4, y: 2)
        .padding(20)
        .accessibilityLabel("Add an item")
        .accessibilityIdentifier("item.add")
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
            HStack(spacing: 8) {
                Text(item.name)
                    .strikethrough(item.isDone)
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                    // The name never gives way. When a row is too narrow — a long name,
                    // a large Dynamic Type size, six categories — the marks are what
                    // should go, not the word that says what to buy.
                    .layoutPriority(1)

                // Every tag the item carries, in the order this list is walked. The
                // first is the one that put the row where it is; the rest are true of
                // it too, and hiding them made a row filed under three things look
                // exactly like one filed under one.
                //
                // Emoji alone, unstyled. Names and capsules beside every row are a
                // second column of text on a screen already showing the name that
                // matters, and each emoji says the same thing in one glyph. The names
                // are still spoken -- see `spoken(_:)` -- so nothing is lost to anyone
                // reading by ear rather than by eye.
                let filed = tagsOn(item, in: tags)
                if !filed.isEmpty {
                    Text(filed.map(\.mark).joined(separator: " "))
                        .font(.callout)
                        // One line, and the ones that do not fit become an ellipsis
                        // rather than being squeezed or wrapped. The Mac needs a
                        // layout of its own for this because it drops names first and
                        // then marks — two different view trees. Here there were never
                        // any names to drop, so a run of marks in one Text is already
                        // the whole answer, and truncation comes for free and never
                        // splits a glyph.
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .accessibilityHidden(true)
                }

                Spacer(minLength: 4)

                // Quietly, and on the row itself. A change that has not been sent is a
                // detail about that line, not news about the app -- and somebody in a
                // shop with no signal would have every line marked, which is a banner
                // by another name.
                if unsent.contains(item.uuid) {
                    Image(systemName: "clock")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .accessibilityLabel("Waiting to be sent")
                }

                if let measure = item.measure(units: unitNames) {
                    Text(measure)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                        // Never squeezed. At an accessibility text size a row can run
                        // out of width entirely, and a Text with no floor of its own is
                        // compressed until it wraps -- which for "1 pack" meant one
                        // letter per line, reading down the side of the row.
                        //
                        // The order of surrender is now the same at every size: the
                        // marks truncate first, then the name wraps, and the amount
                        // keeps the width it needs. It is the shortest thing on the row
                        // and the one nobody can guess from context.
                        .fixedSize(horizontal: true, vertical: false)
                        .layoutPriority(1)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(spoken(item))
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

    /// What the row says when it is read aloud rather than looked at.
    ///
    /// Strikethrough and a grey chip are not information to a screen reader, and the
    /// chip is hidden from it so that it does not arrive as a loose word.
    private func spoken(_ item: Item) -> String {
        let measure = item.measure(units: unitNames).map { ", \($0)" } ?? ""
        let named = tagsOn(item, in: tags).map(\.name)
        let filed = named.isEmpty ? "" : ", in \(named.joined(separator: ", "))"
        let state = item.isDone ? ", crossed off" : ""
        return "\(item.name)\(measure)\(filed)\(state)"
    }

    // MARK: - Doing things

    private func add() async {
        let typed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !typed.isEmpty else { return }

        // Cleared before the request rather than after, so the next item can be typed
        // straight away — the same reason the web form sits outside the swap. Putting
        // the cursor back is the sheet's business now; this only clears what it holds.
        line = ""
        suggestions.clear()

        // The row appears at once, under a uuid minted here and a **negative id**. The
        // negative id never leaves the device: it is a placeholder so the screen has
        // something to key on, and the uuid is what the operation actually names. When
        // the add lands, the reload replaces it with the server's row — same uuid, real
        // id.
        //
        // The name shown until then is the line as typed, near enough. `2 kg apples` is
        // parsed on the server, so the amount and unit arrive with the reload; guessing
        // them here would be a second parser to disagree with the first.
        let uuid = UUID().uuidString.lowercased()
        let local = Item(
            id: -Int64(Date().timeIntervalSince1970 * 1000),
            uuid: uuid,
            name: typed,
            amount: 1,
            unitID: nil,
            doneAt: nil,
            tagIDs: []
        )
        cache.outbox.add(uuid: uuid, localID: local.id, line: typed, on: list)
        show { $0 + [local] }
        await drain()
    }

    /// Opens the editor, once we know what the item is already filed under.
    ///
    /// Asking the server is better — it knows about tags added from another device —
    /// but it must not be a *precondition*. Editing an item is a thing somebody does
    /// standing in a shop with no signal, and on a device with no server it is a thing
    /// they would otherwise never be able to do at all: the editor simply refused to
    /// open, which is the bug this comment exists because of.
    ///
    /// So a failure falls back to what the row already says it is filed under. That is
    /// what the screen is showing anyway, so the editor opens agreeing with the list
    /// behind it.
    private func beginEditing(_ item: Item) async {
        let attached: [Tag]
        do {
            attached = try await api.tags(on: item, in: list)
        } catch let problem as APIError {
            // A refused account is still worth acting on: it is not a connection
            // problem and asking again will not fix it.
            if case .unauthorized = problem {
                identity.signOut()
                return
            }
            if case .notAdmitted = problem {
                identity.signOut(because: problem.localizedDescription)
                return
            }
            attached = tags.filter { item.tagIDs.contains($0.id) }
        } catch {
            attached = tags.filter { item.tagIDs.contains($0.id) }
        }

        editing = Editing(item: item, attached: attached)
    }

    /// Saves an edit: the fields, then the tags that changed.
    ///
    /// Tags have their own routes rather than being part of the update, so this is
    /// the diff. Only what changed is sent -- re-attaching a tag an item already has
    /// would be a conflict, and detaching one it never had a miss.
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
        // They are the last operations without an offline path; see docs/offline.md.
        let before = Set(target.attached.map(\.id))
        for tag in tags where edit.tagIDs.contains(tag.id) && !before.contains(tag.id) {
            try await api.attach(tag, to: target.item, on: list)
        }
        for tag in target.attached where !edit.tagIDs.contains(tag.id) {
            try await api.detach(tag, from: target.item, on: list)
        }
    }

    /// Crosses something off, or puts it back, whether or not there is a connection.
    ///
    /// The screen changes first and the server is told second. That order is the whole
    /// of offline editing: a tick in a shop with no signal is a decision the person has
    /// already made, and an app that waits for a server before showing it has made them
    /// wait for something they cannot influence.
    ///
    /// The queue is what makes the promise good. If the send fails the operation stays
    /// in it, and the next drain — on the next load, or the next time this screen opens
    /// — sends it.
    private func toggle(_ item: Item) async {
        guard list.mayEdit else { return }

        let done = !item.isDone
        cache.outbox.setDone(item, on: list, done: done)
        show { rows in rows.map { $0.uuid == item.uuid ? $0.withDone(done) : $0 } }
        await drain()
    }

    /// Sends what is queued, then says what became of it.
    ///
    /// Only the losses are said out loud. "Three changes sent" is news about plumbing;
    /// "the thing you crossed off had been deleted" is news about the list, and it is
    /// the one case where somebody watched themselves do something that did not happen.
    ///
    /// Called after every successful load, which is what makes the queue drain on its
    /// own: coming back into signal reconnects the change stream, the stream triggers a
    /// load, and the load sends what has been waiting. Nobody has to reopen the screen.
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
        // A drain that sent nothing while something was queued is the other way to
        // learn there is no connection, and often the first: it does not wait for a
        // reload to fail.
        if drained.sent > 0 {
            offline = false
        } else if drained.waiting > 0 && !drained.refused {
            offline = true
        }
        if let lost = drained.lost.first { error = lost }
        // Read back what the server made of it — which is also how a row created here
        // gets its real id. Re-entry stops at the guard above: the queue is empty now.
        if drained.sent > 0 { await load() }
    }

    /// Tries the queue again, every so often, for as long as anything is in it.
    ///
    /// A load drains on success, and a load happens when the change stream reconnects —
    /// which is the right moment when there is a stream to reconnect. It is the wrong
    /// thing to depend on entirely: a queue is work somebody is waiting for, and hanging
    /// it on somebody else editing the list means a tick made in a shop can sit there
    /// until that happens.
    private func keepTrying() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(10))
            await drain()
        }
    }

    private func refreshUnsent() {
        let queued = cache.outbox.forList(list)
        unsent = Set(queued.map(\.itemUUID))
        waiting = queued.count
    }

    /// Rewrites what is on screen, and remembers it.
    ///
    /// One place, so an optimistic change cannot end up on the screen but not in the
    /// cache — which is how a change survives the app being killed before it is sent.
    private func show(_ change: ([Item]) -> [Item]) {
        items = change(items)
        cache.remember(items: items, on: list)
        refreshUnsent()
    }

    /// The server's answer with this device's unsent changes laid back over it.
    ///
    /// Without this a successful load would visibly undo a tick that is still queued —
    /// the server has not been told, so it answers with the old state, and the row
    /// would flick back for as long as the queue is stuck.
    /// The server's answer with this device's unsent changes laid back over it.
    ///
    /// Without this a successful load would visibly undo work that is still queued: the
    /// server has not been told, so it answers with the old state, and the rows would
    /// flick back for as long as the queue is stuck.
    ///
    /// Rows this device created and has not sent are not in the server's answer at all,
    /// so they are carried across from what is already on screen rather than rebuilt.
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
                rows = rows.map {
                    $0.uuid == operation.itemUUID ? $0.withDone(operation.done) : $0
                }

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

    /// Empties the trolley of what is on this screen, and says so on the wire.
    ///
    /// The rows are named rather than described. "Everything that is done" replayed an
    /// hour later would also take what somebody else ticked off meanwhile, which nobody
    /// asked for — `docs/offline.md` (4).
    private func clearDone() async {
        guard list.mayEdit, !done.isEmpty else { return }
        let swept = done
        cache.outbox.clearDone(swept, on: list)
        show { rows in rows.filter { row in !swept.contains { $0.uuid == row.uuid } } }
        await drain()
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
                // retrying it forever would hammer the server while signed out. The
                // same goes for a refusal: reconnecting every three seconds to be
                // told no again is a loop nothing ends.
                if case .unauthorized = problem {
                    identity.signOut()
                    return
                }
                if case .forbidden = problem { return }
                if case .notAdmitted = problem {
                    identity.signOut(because: problem.localizedDescription)
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
            } else if case .notAdmitted = problem {
                identity.signOut(because: problem.localizedDescription)
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
            let (fetchedUnits, fetchedTags) = try await (units, tags)
            cache.remember(units: fetchedUnits)
            cache.remember(tags: fetchedTags, on: list)
            (self.units, self.tags) = (fetchedUnits, fetchedTags)
        } catch {
            // Not shown: without these, rows lose their measure and their grouping,
            // which is a poorer list rather than no list. `load()` reports what
            // actually stops the screen working.
            //
            // But "poorer" is not good enough on a device that has no server and never
            // will: there every list would have no units and no aisles for ever. So
            // what the server would have said is bundled, and used when it cannot be
            // asked and the cache has nothing either.
            seedReference()
        }
    }

    /// Falls back to the reference set that shipped with the app.
    ///
    /// Written to the cache as well as used, so the next screen finds it without
    /// asking — and so that a device which later gains a server simply overwrites it
    /// with that server's answer, ids and all. The ids are the same ids; see
    /// `Reference`.
    private func seedReference() {
        if units.isEmpty {
            units = Reference.units
            cache.remember(units: units)
        }
        if tags.isEmpty {
            tags = Reference.tags
            cache.remember(tags: tags, on: list)
        }
    }

    /// Puts the list up as it was last seen, before asking anything.
    ///
    /// Units and tags in the same breath: an item read out of the cache with no unit
    /// and no category is a bare name in no aisle, which is a worse answer than the
    /// one the shop actually needs.
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
            } else if case .notAdmitted = problem {
                identity.signOut(because: problem.localizedDescription)
            } else if case .transport = problem {
                // See ListsView.load: no signal is a state, not an event. What is on
                // screen stays there -- it is the last thing the server said.
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

/// Adding items, one after another.
///
/// A sheet rather than a field pinned to the top of the list, so that the screen is
/// the list until somebody asks to add to it — and so that adding works the same way
/// here as it does on the lists screen.
///
/// It stays open after each item. Somebody writing a shopping list writes ten things,
/// not one, and a sheet that closed each time would make the tenth cost ten taps more
/// than the first. What was just added is behind it on the list.
private struct AddItemSheet: View {
    @Binding var line: String
    let suggestions: Suggestions
    let add: () async -> Void

    @Environment(\.dismiss) private var dismiss
    @FocusState private var typing: Bool

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                Section {
                    TextField("Add an item — try 2 kg apples", text: $line)
                        .focused($typing)
                        // `.return` rather than `.done`: the next thing somebody does
                        // is type another item, and `done` on the keyboard reads as
                        // "finished adding".
                        .submitLabel(.return)
                        .onSubmit { Task { await addAndStay() } }
                        .autocorrectionDisabled()
                        .accessibilityIdentifier("item.line")
                }

                // Only what matches. A permanent list of things this list has bought
                // before is a screen of its own, and not the one somebody asked for.
                if !suggestions.offered.isEmpty {
                    Section {
                        ForEach(suggestions.offered, id: \.self) { suggestion in
                            Button {
                                // Fills the field rather than adding outright: what is
                                // typed may carry a quantity, and only the server knows
                                // what a line means.
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
                    } header: {
                        Text("Bought before")
                    }
                }
            }
            .navigationTitle("Add an item")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { line = ""; dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { Task { await addAndStay() } }
                        .disabled(line.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .onAppear { typing = true }
        }
        .presentationDetents([.medium, .large])
    }

    /// Adds, and puts the cursor back for the next one.
    private func addAndStay() async {
        guard !line.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        await add()
        typing = true
    }
}
