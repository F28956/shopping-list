import Foundation
import Observation

/// What is on one list, and everything that can be done to it.
///
/// Out of the views, and that is the whole point. This logic used to live inside
/// `ItemsView` and again inside `MacItemsView` -- nine functions identical to the
/// byte, five more between seventy and ninety-five per cent -- and the copies drifted
/// the moment anything changed. Reordering categories offline was fixed on the phone
/// and left broken on the Mac; so were the synced history, local suggestions, and the
/// unit an edit defaults to. Four in one afternoon.
///
/// It is also why none of it was tested. Reaching `withUnsent` or `drain` meant
/// hosting a view, so nothing did, and the rules about what a queue does with a
/// refusal were verified by hand or not at all. Android had `ItemsViewModel` from the
/// start and this is its counterpart.
///
/// The views keep what is genuinely theirs: what is typed but not yet added, which
/// sheet is open, which row is being edited. Everything about the list itself is here.
@MainActor
@Observable
final class ItemsModel {
    let list: List

    /// Narrowed to what this screen actually asks for: shopping, and the queue it
    /// empties. No accounts and no sharing -- that is the point of the split, see
    /// ``Backend``. A device with no server can answer every one of these, which is
    /// what makes a local conformer possible later.
    private let api: any Backend & Destination
    private let cache: Cache

    /// Says this person is no longer signed in, and why if there is a reason.
    ///
    /// A closure rather than a reach for `Identity`: signing out is the app's to do —
    /// it tears down the screen this object is attached to — and a model that could do
    /// it directly would be a model that can outlive its own caller.
    ///
    /// Settable rather than passed in, because the identity is an environment value
    /// and a view has none of those when its state is built. The screen sets it as it
    /// appears. Unset, a refusal is simply not acted on, which is the safe way round.
    var signedOut: ((String?) -> Void)?

    // MARK: - What is on screen

    var items: [Item] = []
    var units: [Unit] = []
    var tags: [Tag] = []
    var total: Int64 = 0
    var truncated = false

    /// Whether anything has been shown yet. The spinner is on until it has.
    var loaded = false
    /// Whether the far end has ever answered. What earns an empty state: a failed load
    /// with nothing cached is not evidence that somebody has no items.
    var fresh = false
    /// See `ListsView.offline`.
    var offline = false
    /// How many changes made here are still waiting to be sent.
    var waiting = 0
    /// The rows carrying one of them. Marked on the row itself rather than with a
    /// banner: it is a detail about that line, not news about the app.
    var unsent: Set<String> = []
    /// Something was refused and will not retry itself. The one state of the three in
    /// `docs/offline.md` that is worth interrupting somebody for.
    var refused = false
    var error: String?

    /// What has been typed and not yet added.
    ///
    /// Here rather than in the view because the suggestions are a function of it and
    /// `add` consumes it — three things that have to agree, and agreeing is easier in
    /// one place. The field binds to it.
    var line = ""

    /// The row whose editor is open, once we know what it is already filed under.
    ///
    /// Set by `beginEditing`, which asks before the sheet opens so the editor never
    /// renders a category section it is about to change under a thumb.
    var editing: Editing?

    let suggestions = Suggestions()

    /// Guards against a drain and a reload calling each other round in a circle.
    private var draining = false

    /// An item and what it is already filed under, fetched before the sheet opens so
    /// the editor never renders a tag section it is about to change under your thumb.
    struct Editing: Identifiable {
        let item: Item
        let attached: [Tag]
        var id: Int64 { item.id }
    }
    /// What keeps this screen level with the database.
    ///
    /// Not a notification any more. That protocol -- write, announce, listen, re-read
    /// the right things -- was four steps, each skippable in silence, and removing a
    /// category skipped three of them at once: four of the six writers never announced,
    /// the one listener re-read only the items, and three screens of four were not
    /// listening at all.
    ///
    /// `ValueObservation` removes the protocol instead of documenting it. It knows which
    /// tables the fetch read and re-runs it when any of them changes, whoever changed
    /// them and from wherever. There is nothing for a writer to remember, nothing for a
    /// screen to subscribe to, and no list of properties here to keep in step with the
    /// ones the fetch returns -- because the fetch returns all of them.
    ///
    /// Started here rather than left to a `.task` in a view, for the reason the last
    /// version of this went wrong: three screens of four forgot to ask.
    ///
    /// `nonisolated(unsafe)` because `deinit` is not on the main actor and has to cancel
    /// this. Safe by construction: written once in `init`, read once in `deinit`.
    nonisolated(unsafe) private var watching: Task<Void, Never>?

    init(list: List, api: any Backend & Destination, cache: Cache = .shared) {
        self.list = list
        self.api = api
        self.cache = cache

        watching = Task { [weak self] in
            while !Task.isCancelled {
                guard let stream = cache.observe(list: list) else { return }
                do {
                    for try await contents in stream {
                        guard let self else { return }
                        self.adopt(contents)
                    }
                    // Ended without erroring: the database is gone, and nothing will
                    // change again. Restarting would spin.
                    return
                } catch {
                    // An observation that stops is a screen that quietly stops updating
                    // -- exactly the failure this whole change exists to remove, and
                    // worse than the old notification because it would look like it was
                    // still working. So it is started again rather than given up on.
                    try? await Task.sleep(for: .seconds(1))
                }
            }
        }
    }

    deinit {
        watching?.cancel()
    }

    /// What the database currently says, put on screen.
    ///
    /// The guards are about emptiness rather than staleness: a cache not filled yet must
    /// not blank a screen the server has already answered. Once there is anything there
    /// it wins, because this only runs when something wrote.
    private func adopt(_ contents: Cache.ListContents) {
        if !contents.items.isEmpty || items.isEmpty {
            items = contents.items
            if !fresh { total = Int64(contents.items.count) }
        }
        if !contents.units.isEmpty { units = contents.units }
        if !contents.tags.isEmpty { tags = contents.tags }
        if !items.isEmpty { loaded = true }
        refreshUnsent()
    }

    /// The rows to show, in the order the shop is walked.
    ///
    /// The tag that decides the order rides on the row instead of a heading above it:
    /// a heading says the same thing as the chip beneath it, and one of the two is
    /// redundant on a screen this narrow.
    var outstanding: [Item] { items.filter { !$0.isDone } }
    var ordered: [Item] { grouped(outstanding, by: tags).flatMap(\.items) }
    var done: [Item] { items.filter(\.isDone) }

    /// Units by id, which is what a row needs to spell a measure.
    var unitNames: [Int64: String] {
        Dictionary(uniqueKeysWithValues: units.map { ($0.id, $0.name) })
    }

    // MARK: - Doing things

    /// Asks again for what has just been typed.
    ///
    /// The server's history when there is a server, and this device's own when there
    /// is not. Autocomplete used to be the server's alone, so a device with none
    /// offered nothing at all — and the history that makes a re-typed `milk` arrive in
    /// pints under dairy had nowhere to live.
    ///
    /// Falling back on a failure as well as on absence: a history that vanishes in a
    /// shop with no signal is missing exactly when somebody is typing one-handed.
    func suggest(_ typed: String) {
        suggestions.update(typed: typed) { [self] wanted in
            guard !ServerDirectory.isOnDeviceOnly else {
                return QuickAdd.suggest(wanted, from: cache.history(on: list))
            }
            do {
                return try await api.suggestions(matching: wanted, on: list)
            } catch {
                return QuickAdd.suggest(wanted, from: cache.history(on: list))
            }
        }
    }

    /// Something changed the cache from outside this screen.
    ///
    /// In practice a tick that arrived from the watch, which lands in the cache with no
    /// view involved. Without this the phone sat there showing the row un-ticked while
    /// its own database said otherwise, which looked exactly like the watch failing.
    ///
    /// Safe against the writes this screen makes itself: re-reading is idempotent and
    /// does not write back, so `show` → cache → here → `items` settles in one pass with
    /// nothing to announce.
    /// Re-reads everything this screen holds.
    ///
    /// Kept for the tests, which drive it directly rather than waiting on an
    /// observation. In the app nothing calls it: the observation started in `init` does
    /// this whenever the database moves, which is the whole point of the change.
    func reloadFromCache() {
        guard let contents = cache.contents(of: list) else { return }
        adopt(contents)
    }


    func add() async {
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
        // **What the line does is not decided here.** Which unit a bare name lands in,
        // whether `Milk` is the `milk` already on the list, whether a crossed-off row
        // comes back — all of it is `parsing::add`, compiled in and shared with the
        // server. Written out in Swift instead, these drifted immediately: this screen
        // showed three rows for `milk`, `milk` and `Milk` where a server would have
        // shown one, and nothing was ever going to merge them.
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
            // Nothing is created. Queued all the same, and under a **fresh** uuid: the
            // server runs this same rule when it hears the line, and handing it the
            // row's own uuid takes the early return in `create` and skips the putting
            // back.
            guard let alike = items.first(where: { $0.uuid == uuid }) else { return }
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

        case .new(let row):
            let uuid = UUID().uuidString.lowercased()
            let local = Item(
                id: -Int64(Date().timeIntervalSince1970 * 1000),
                uuid: uuid,
                name: row.name,
                amount: row.amount,
                unitID: row.unitID,
                doneAt: nil,
                // Where the history says it belongs, so a re-added item files itself.
                tagIDs: row.tagIDs
            )
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
            cache.remember(local, on: list, isNew: true)
            show { $0 + [local] }
        }

        await drain()
    }

    /// Records the order this person walks this list in.
    ///
    /// Written down here first, and queued rather than sent. It used to go straight at
    /// the server and put "Something went wrong" on screen when it could not get there
    /// -- which on a device with no server was every time, so rearranging the aisles
    /// was a control that always failed.
    ///
    /// The order is this person's, so nothing has to be merged: the queue carries it to
    /// a server if there ever is one, and until then the cache is the only place it
    /// needs to exist.
    func reorder(_ chosen: [Tag]) async {
        tags = chosen
        cache.remember(tags: chosen, on: list)
        cache.outbox.setTagOrder(chosen, on: list)
        show { $0 }
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
    func beginEditing(_ item: Item) async {
        let attached: [Tag]
        do {
            attached = try await api.tags(on: item, in: list)
        } catch let problem as APIError {
            // A refused account is still worth acting on: it is not a connection
            // problem and asking again will not fix it.
            if case .unauthorized = problem {
                signedOut?(nil)
                return
            }
            if case .notAdmitted = problem {
                signedOut?(problem.localizedDescription)
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
    func apply(_ edit: ItemEdit, to target: Editing) async throws {
        // Counted rather than measured is still a unit, and `unit` is the one that
        // says so. The server normalises this on its own update and the device has to
        // agree: left empty, `milk` and `1 unit milk` are different units and so
        // different rows, and the list grows a near-duplicate nothing will merge.
        //
        // It is also what a row shows. An item with no unit prints no measure at all,
        // which reads as a row that has lost one rather than a row of one thing.
        let unitID = edit.unitID ?? units.first { $0.name.lowercased() == "unit" }?.id
        // A missing amount is one. Nobody writing a shopping list means "zero of it".
        let amount = edit.amount > 0 ? edit.amount : 1

        cache.outbox.update(
            target.item,
            on: list,
            name: edit.name,
            amount: amount,
            unitID: unitID
        )
        // Queued, not sent. Filing used to go straight to the network and fail if it
        // could not get there, which offline lost the change and on a device with no
        // server meant the aisle picker never worked at all.
        let before = Set(target.attached.map(\.id))
        for tag in tags where edit.tagIDs.contains(tag.id) && !before.contains(tag.id) {
            cache.outbox.tag(target.item, on: list, tagID: tag.id, attached: true)
        }
        for tag in target.attached where !edit.tagIDs.contains(tag.id) {
            cache.outbox.tag(target.item, on: list, tagID: tag.id, attached: false)
        }

        // What somebody corrected an item *to* is a better memory than what they first
        // typed, which is why the server records history here as well as on an add.
        // The count does not rise: editing one row twice is one intention.
        cache.remember(
            Item(
                id: target.item.id,
                uuid: target.item.uuid,
                name: edit.name,
                amount: amount,
                unitID: unitID,
                doneAt: target.item.doneAt,
                tagIDs: Array(edit.tagIDs)
            ),
            on: list,
            isNew: false
        )

        show { rows in
            rows.map {
                guard $0.uuid == target.item.uuid else { return $0 }
                return Item(
                    id: $0.id,
                    uuid: $0.uuid,
                    name: edit.name,
                    amount: amount,
                    unitID: unitID,
                    doneAt: $0.doneAt,
                    // What the editor was closed on. It used to keep the row's old
                    // filing and wait for the server to send the new one back, which
                    // on a device with no server was a wait that never ended.
                    tagIDs: Array(edit.tagIDs)
                )
            }
        }
        await drain()
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
    func toggle(_ item: Item) async {
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
    func drain() async {
        guard !draining else { return }

        // Read the queue back even when there is nothing to send, and *before* the
        // early return. The lists screen drains the same queue on its own — it has to,
        // because the app opens there — so this screen's count can go stale the moment
        // that happens. Returning early without refreshing left "3 changes waiting to
        // be sent" on a screen whose queue had been empty for minutes.
        refreshUnsent()
        cache.handOverIfNeeded()
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
    func keepTrying() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(10))
            await drain()
        }
    }

    /// Which rows are still waiting on a server, and how many.
    ///
    /// Both are empty on a device with no server, and not because nothing is queued --
    /// on a list that is only ever this device's, *everything* is queued and nothing
    /// ever leaves. Marking every row as waiting says the app is behind on work it
    /// means to do, and it isn't: the list is already exactly what it should be. The
    /// queue is still kept, because a server added later is owed every one of them.
    func refreshUnsent() {
        guard !ServerDirectory.isOnDeviceOnly else {
            unsent = []
            waiting = 0
            return
        }
        let queued = cache.outbox.forList(list)
        unsent = Set(queued.map(\.itemUUID))
        waiting = queued.count
    }

    /// Rewrites what is on screen, and remembers it.
    ///
    /// One place, so an optimistic change cannot end up on the screen but not in the
    /// cache — which is how a change survives the app being killed before it is sent.
    func show(_ change: ([Item]) -> [Item]) {
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
    func withUnsent(_ fromServer: [Item]) -> [Item] {
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

            case QueuedOperation.Kind.attachTag, QueuedOperation.Kind.detachTag:
                guard let tagID = operation.tagID else { break }
                let attaching = operation.kind == QueuedOperation.Kind.attachTag
                rows = rows.map { row in
                    guard row.uuid == operation.itemUUID else { return row }
                    var filed = row.tagIDs.filter { $0 != tagID }
                    if attaching { filed.append(tagID) }
                    return Item(
                        id: row.id,
                        uuid: row.uuid,
                        name: row.name,
                        amount: row.amount,
                        unitID: row.unitID,
                        doneAt: row.doneAt,
                        tagIDs: filed
                    )
                }

            default:
                break
            }
        }
        return rows
    }

    func remove(_ item: Item) async {
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
    func clearDone() async {
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
    func watch() async {
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
                    signedOut?(nil)
                    return
                }
                if case .forbidden = problem { return }
                if case .notAdmitted = problem {
                    signedOut?(problem.localizedDescription)
                    return
                }
            } catch {
                // Anything else is the connection going away -- a tunnel, a lock
                // screen, a server restarting. Ordinary, and not worth showing; the
                // wait below and the loop are the whole response.
            }

            // Waiting keeps a server that is refusing everything from being asked as
            // fast as the loop can go round.
            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
        }
    }

    /// Runs something that changes the list, then reloads.
    ///
    /// Reloading rather than patching the array in place: the server decides the order
    /// and what a line meant, and guessing at either is how a phone comes to disagree
    /// with the browser about what is on the list.
    func attempt(_ work: () async throws -> Void) async {
        do {
            try await work()
            await load()
        } catch let problem as APIError {
            if case .unauthorized = problem {
                signedOut?(nil)
            } else if case .notAdmitted = problem {
                signedOut?(problem.localizedDescription)
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
    func loadReference() async {
        do {
            async let units = api.units()
            async let tags = api.tags(orderedFor: list)
            // Alongside them, and for the same reason: it is what this screen needs to
            // read a typed line the way the server would. It changes when somebody
            // shops, not when somebody ticks something off, so it belongs here rather
            // than in `load` -- see `Cache.adopt(history:on:)`.
            async let remembered = api.history(on: list)
            let (fetchedUnits, fetchedTags) = try await (units, tags)
            cache.remember(units: fetchedUnits)
            cache.remember(tags: fetchedTags, on: list)
            (self.units, self.tags) = (fetchedUnits, fetchedTags)

            // After the two above, and allowed to fail on its own: a server that
            // predates this route still gives units and aisles, and a device without
            // the memory resolves lines a little less well rather than not at all.
            if let remembered = try? await remembered {
                cache.adopt(history: remembered, on: list)
            }
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
    func seedReference() {
        // The cache first, and only then the bundle. Checking the *view's* state was
        // the bug: this runs from its own `.task`, which races the one that fills that
        // state from the cache, so it usually found it empty and wrote the shipped
        // order over the top. On a device with no server that quietly undid the aisle
        // order somebody had arranged, every time they opened a list.
        //
        // The bundled set is a first run and nothing else. Once anything is stored --
        // a server's answer, or a walk somebody chose -- that is the authority.
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

    /// Puts the list up as it was last seen, before asking anything.
    ///
    /// The observation's first value does this on its own, immediately, so a screen is
    /// never blank while a request is in flight without anybody arranging it. This
    /// remains because the callers read better for saying it, and because it is
    /// synchronous: a view appearing wants the rows in the same turn, not one hop later.
    func showWhatWeHave() {
        guard !fresh else { return }
        reloadFromCache()
    }

    func load() async {
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
                signedOut?(nil)
            } else if case .notAdmitted = problem {
                signedOut?(problem.localizedDescription)
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
