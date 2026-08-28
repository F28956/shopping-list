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

    /// Everything about the list itself — see `ItemsModel`, which the phone shares.
    @State private var model: ItemsModel

    /// There is no server. The default — see `ServerDirectory`.
    @Environment(\.capabilities) private var capabilities

    // What is genuinely this window's: which sheet is open.
    @State private var confirmingClear = false
    @State private var ordering = false

    @FocusState private var typing: Bool

    /// `backend` is the one the lists screen chose -- the device's own server, or a
    /// server with the cache and the queue behind it. Passed down rather than rebuilt,
    /// because two screens deciding separately is two screens that can disagree, and on
    /// a migrated device the disagreement is a list that opens empty.
    init(api: API, backend: any Backend, list: List) {
        self.api = api
        self.list = list
        _model = State(initialValue: ItemsModel(list: list, api: backend))
    }


    private var outstanding: [Item] { model.items.filter { !$0.isDone } }
    private var done: [Item] { model.items.filter(\.isDone) }
    /// Outstanding items in the order the shop is walked, with no headings.
    ///
    /// The categories decide the order and then get out of the way: what tells you
    /// where a thing lives is the tag on its own row, not a band across the list.
    private var ordered: [Item] { grouped(model.outstanding, by: model.tags).flatMap(\.items) }

    private var tagsByID: [Int64: Tag] {
        Dictionary(uniqueKeysWithValues: model.tags.map { ($0.id, $0) })
    }
    private var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: model.units.map { ($0.id, $0.name) })
    }

    var body: some View {
        SwiftUI.List {
            if model.truncated {
                Text("Showing \(model.items.count) of \(Int(model.total)). This list is long enough to be worth splitting.")
                    .accessibilityIdentifier("truncation.notice")
                    .accessibilityLabel(
                        "Showing \(model.items.count) of \(Int(model.total)). "
                            + "This list is long enough to be worth splitting."
                    )
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            // Not on a Mac with no server: everything is queued there and nothing
            // ever leaves, so a permanent "2 changes waiting to be sent" would be
            // reporting the arrangement rather than a problem.
            if (model.offline || model.waiting > 0 || model.refused) && capabilities.syncing {
                OfflineNote(offline: model.offline, waiting: model.waiting, refused: model.refused)
            }

            // "Nothing on this list yet" is a claim, and after a load that failed
            // with nothing cached it is a claim nobody has checked.
            if model.items.isEmpty && model.loaded && !model.fresh && capabilities.syncing {
                Text(
                    model.offline
                        ? "Can't reach the server. This list will appear as soon as there is a connection."
                        : "Couldn't load this list. What is on it is not known yet."
                )
                .foregroundStyle(.secondary)
            } else if model.outstanding.isEmpty {
                Text(model.items.isEmpty ? "Nothing on this list yet." : "All done.")
                    .foregroundStyle(.secondary)
            }

            ForEach(model.ordered) { row($0) }

            if !model.done.isEmpty {
                Section {
                    ForEach(model.done) { row($0) }
                } header: {
                    HStack {
                        Text("\(model.done.count) done")
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
        .onChange(of: model.line) { _, typed in
            model.suggestions.update(typed: typed) { wanted in
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
            if capabilities.sharing {
                if #available(macOS 26.0, *) {
                    ToolbarItem(placement: .primaryAction) {
                        StatusDot(waiting: model.waiting, offline: model.offline)
                    }
                    .sharedBackgroundVisibility(.hidden)
                } else {
                    ToolbarItem(placement: .primaryAction) {
                        StatusDot(waiting: model.waiting, offline: model.offline)
                    }
                }
            }
        }
        .sheet(isPresented: $ordering) {
            TagOrderSheet(
                list: list,
                tags: model.tags,
                // What the list's items actually carry, so the sheet can say which
                // of twenty-one names are the ones that would change anything.
                inUse: Set(model.items.flatMap(\.tagIDs))
            ) { chosen in
                // The model's, which writes it down first and queues it. This used to
                // go straight at the server and put "Something went wrong" on screen
                // when it could not get there -- which with no server was every time.
                // The phone was fixed and this was not, which is the whole argument
                // for there being one of these rather than two.
                await model.reorder(chosen)
            }
        }
        // Settings changes this under our feet, and storage is not observable state.
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
        }
        .task { await model.loadReference() }
        .task {
            // Set here rather than passed in: the identity is an environment value and
            // a view has none of those when its state is built. See `ItemsModel`.
            model.signedOut = { because in
                if let because {
                    identity.signOut(because: because)
                } else {
                    identity.signOut()
                }
            }
            await model.refreshUnsent()
            await model.load()
        }
        .task { await model.keepTrying() }
        .task { await model.watch() }
        .sheet(item: $model.editing) { target in
            MacItemEditor(
                item: target.item,
                units: model.units,
                tags: model.tags,
                attached: target.attached
            ) { edit in
                await model.attempt { try await model.apply(edit, to: target) }
            }
        }
        .confirmationDialog(
            "Clear \(model.done.count) done \(model.done.count == 1 ? "item" : "items")?",
            isPresented: $confirmingClear
        ) {
            Button("Clear", role: .destructive) { Task { await model.clearDone() } }
            Button("Cancel", role: .cancel) {}
        }
        .alert("Something went wrong", isPresented: .constant(model.error != nil)) {
            Button("OK") { model.error = nil }
        } message: {
            Text(model.error ?? "")
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
                if typing && !model.suggestions.offered.isEmpty {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(model.suggestions.offered, id: \.self) { suggestion in
                            Button {
                                // Added outright, as on the phone: picking something
                                // you have bought before is already the whole
                                // decision, and a second press to confirm it asks
                                // nothing. It goes through the same resolve as
                                // anything typed, so it arrives measured and filed
                                // the way it was last time.
                                model.line = suggestion
                                typing = true
                                // What was accepted is no longer a suggestion.
                                model.suggestions.clear()
                                Task { await model.add() }
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
                    TextField("Add an item — try 2 kg apples", text: $model.line)
                        .accessibilityIdentifier("model.add.field")
                        .textFieldStyle(.roundedBorder)
                        .controlSize(.large)
                        .focused($typing)
                        .onSubmit { Task { await model.add() } }

                    Button("Add") { Task { await model.add() } }
                        .accessibilityIdentifier("model.add.button")
                        .buttonStyle(.borderedProminent)
                        .disabled(model.line.trimmingCharacters(in: .whitespaces).isEmpty)
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
                Task { await model.beginEditing(item) }
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
                    if model.unsent.contains(item.uuid) {
                        Image(systemName: "clock")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Waiting to be sent")
                    }

                    Spacer(minLength: 8)

                    if let measure = item.measure(units: model.unitNames) {
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
                Button("Edit…") { Task { await model.beginEditing(item) } }
                Button(item.isDone ? "Put back" : "Cross off") {
                    Task { await model.toggle(item) }
                }
                Divider()
                Button("Delete", role: .destructive) { Task { await model.remove(item) } }
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
            set: { _ in Task { await model.toggle(item) } }
        )
    }


    /// What the row says when it is read aloud rather than looked at.
    ///
    /// Struck-through text and grey are not information to a screen reader, and the
    /// measure sits in a separate label it would read as a loose number.
    private func accessibleName(_ item: Item) -> String {
        let measure = item.measure(units: model.unitNames).map { ", \($0)" } ?? ""
        let state = item.isDone ? ", crossed off" : ""
        // Spoken here rather than by the chips, which are hidden from VoiceOver: read
        // separately they arrive as loose words after the item with nothing to say
        // what they are.
        let filed = item.tagIDs.compactMap { tagsByID[$0]?.name }
        let under = filed.isEmpty ? "" : ", in \(filed.joined(separator: ", "))"
        return "\(item.name)\(measure)\(under)\(state)"
    }

}
