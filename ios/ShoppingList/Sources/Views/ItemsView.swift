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

    /// Everything about the list itself — see `ItemsModel`, which the Mac shares.
    @State private var model: ItemsModel

    // What is genuinely this screen's: which sheet is open. The rest is the model's.
    @State private var adding = false
    @State private var confirmingClear = false
    @State private var ordering = false
    @State private var sharing = false

    /// `backend` is the one the lists screen chose -- the device's own server, or a
    /// server with the cache and the queue behind it. Passed down rather than rebuilt,
    /// because two screens deciding separately is two screens that can disagree, and on
    /// a migrated device the disagreement is a list that opens empty.
    init(api: API, backend: any Backend, list: List) {
        self.api = api
        self.list = list
        _model = State(initialValue: ItemsModel(list: list, api: backend))
    }

    var body: some View {
        SwiftUI.List {
            // Nothing to say on a device kept to itself: nothing is stale, nothing is
            // waiting for a connection that is coming, and a line apologising for one
            // somebody declined is worse than silence.
            if (model.offline || model.waiting > 0 || model.refused) && !ServerDirectory.isOnDeviceOnly {
                Section { OfflineNote(offline: model.offline, waiting: model.waiting, refused: model.refused) }
            }

            if model.truncated {
                Section { truncationNotice }
            }

            // "Nothing on this list yet" is a claim, and with nothing cached and a
            // model.load that failed it is a claim nobody has checked. `fresh` is what earns
            // it, and only the server can set that.
            //
            // Except on a device kept to itself, where there is no server to have
            // checked with and this device is the only thing that could know. There,
            // empty means empty.
            if model.items.isEmpty && model.loaded && !model.fresh && !ServerDirectory.isOnDeviceOnly {
                Section {
                    Text(
                        model.offline
                            ? "Can't reach the server. This list will appear as soon as there is a connection."
                            : "Couldn't model.load this list. What is on it is not known yet."
                    )
                    .foregroundStyle(.secondary)
                }
            } else if model.outstanding.isEmpty && model.loaded {
                Section {
                    Text(model.items.isEmpty ? "Nothing on this list yet." : "All model.done.")
                        .foregroundStyle(.secondary)
                }
            }

            Section {
                ForEach(model.ordered) { item in
                    row(item)
                }
            }

            // What is already in the trolley, out of the way of what is not.
            if !model.done.isEmpty {
                Section {
                    ForEach(model.done) { item in
                        row(item)
                    }
                } header: {
                    doneHeader
                }
            }
        }
        .onChange(of: model.line) { _, typed in
            model.suggest(typed)
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
                        waiting: model.waiting,
                        offline: model.offline,
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
            AddItemSheet(line: $model.line, suggestions: model.suggestions) { await model.add() }
        }
        .sheet(isPresented: $sharing) {
            ShareSheet(list: list, api: api) {}
        }
        .sheet(isPresented: $ordering) {
            TagOrderSheet(
                list: list,
                tags: model.tags,
                // What the list's items actually carry, so the sheet can say which
                // of twenty-one names are the ones that would change anything.
                inUse: Set(model.items.flatMap(\.tagIDs))
            ) { chosen in
                await model.reorder(chosen)
            }
        }
        .refreshable { await model.load() }
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
            // `load` drains on success, so what was queued in the shop yesterday goes
            // as soon as the first request gets through.
            await model.load()
        }
        .task { await model.watch() }
        .task { await model.keepTrying() }
        // Coming back from the background is the one gap the stream cannot cover:
        // iOS tears the connection down and the reconnect has not happened yet.
        .onChange(of: phase) { _, now in
            if now == .active { Task { await model.load() } }
        }
        .sheet(item: $model.editing) { target in
            ItemEditor(
                item: target.item,
                units: model.units,
                tags: model.tags,
                attached: target.attached
            ) { edit in
                await model.attempt { try await model.apply(edit, to: target) }
            }
        }
        // Asked rather than assumed: this is the one control on the screen that takes
        // several rows at once, and a mis-tap cannot be undone from here.
        .confirmationDialog(
            "Clear \(model.done.count) model.done \(model.done.count == 1 ? "item" : "model.items")?",
            isPresented: $confirmingClear,
            titleVisibility: .visible
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

    /// The done section's heading, with the one control that empties it.
    ///
    /// Its own property for the same reason as `truncationNotice`: `body` is long
    /// enough that the type-checker gives up on it, and a `Button` whose title is
    /// conditional is exactly the kind of thing it gives up on.
    private var doneHeader: some View {
        HStack {
            Text("\(model.done.count) model.done")
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
        ForEach(model.suggestions.offered, id: \.self) { suggestion in
            Button {
                // Fills the field rather than adding outright. What is typed may
                // carry a quantity -- "2 kg app" -- and the only thing that knows
                // what a line means is the server, so guessing here is how the phone
                // and the browser start disagreeing about it.
                model.line = suggestion
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
        let shown = model.items.count
        let all = Int(model.total)

        return Text("Showing \(shown) of \(all). This list is long enough to be worth splitting.")
            .font(.footnote)
            .foregroundStyle(.secondary)
    }

    private func row(_ item: Item) -> some View {
        Button {
            Task { await model.toggle(item) }
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
                let filed = tagsOn(item, in: model.tags)
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
                if model.unsent.contains(item.uuid) {
                    Image(systemName: "clock")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .accessibilityLabel("Waiting to be sent")
                }

                if let measure = item.measure(units: model.unitNames) {
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
                Task { await model.remove(item) }
            } label: {
                Label("Delete", systemImage: "trash")
            }

            Button {
                Task { await model.beginEditing(item) }
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
        let measure = item.measure(units: model.unitNames).map { ", \($0)" } ?? ""
        let named = tagsOn(item, in: model.tags).map(\.name)
        let filed = named.isEmpty ? "" : ", in \(named.joined(separator: ", "))"
        let state = item.isDone ? ", crossed off" : ""
        return "\(item.name)\(measure)\(filed)\(state)"
    }

}

/// Adding items, one after another.
///
/// A sheet rather than a field pinned to the top of the list, so that the screen is
/// the list until somebody asks to model.add to it — and so that adding works the same way
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

                    // In the sheet rather than the toolbar, now that the toolbar's
                    // right-hand slot is `Done`. A confirming action gets one place in
                    // a bar and this sheet's is taken -- and the button belongs beside
                    // the field it acts on anyway, where a thumb already is.
                    //
                    // The return key does the same thing, and is how most of these get
                    // added; this is for the people who never look for it.
                    Button("Add") { Task { await addAndStay() } }
                        .disabled(line.trimmingCharacters(in: .whitespaces).isEmpty)
                        .accessibilityIdentifier("item.add")
                }

                // Only what matches. A permanent list of things this list has bought
                // before is a screen of its own, and not the one somebody asked for.
                if !suggestions.offered.isEmpty {
                    Section {
                        ForEach(suggestions.offered, id: \.self) { suggestion in
                            Button {
                                // Added outright. It used to fill the field and wait
                                // for `Add`, on the grounds that a line may carry a
                                // quantity and only the server knew what one meant --
                                // which stopped being true when the parser moved into
                                // the app. Picking something you have bought before is
                                // already the whole decision, and a second tap to
                                // confirm it is a tap that asks nothing.
                                //
                                // It goes through the same resolve as anything typed,
                                // so it arrives measured and filed the way it was last
                                // time: `Milk` comes back as a pint, under dairy.
                                line = suggestion
                                Task { await addAndStay() }
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
                // `Done` on the right and nothing on the left, which is the shape for
                // a sheet that commits as it goes. The left slot means *cancel* --
                // dismiss and keep nothing -- and this sheet cannot honour that: by
                // the time somebody leaves it, the items are already on the list.
                // `Done` sitting there was promising an undo that does not exist.
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { line = ""; dismiss() }
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
