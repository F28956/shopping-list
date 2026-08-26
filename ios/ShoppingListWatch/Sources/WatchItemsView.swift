import SwiftUI

/// What is left to get, and one gesture: tap to cross off, tap again to put back.
///
/// There is no adding, no editing, no deleting and no tags. Not because a watch could
/// not show them, but because the moment this screen is for is one hand on a trolley
/// — and a row that does two things is a row that does the wrong one.
///
/// The one thing it *does* do, it does offline. A tick in a shop is a decision already
/// taken, and the watch is the screen most likely to be taking it somewhere with no
/// signal — so it keeps the same cache and the same queue as the phones. It used to
/// throw the tick away and replace the list with an error, which lost the change and
/// the list in one go.
struct WatchItemsView: View {
    let api: API
    let list: List
    @Environment(WatchIdentity.self) private var identity
    @Environment(\.scenePhase) private var phase

    @State private var items: [Item] = []
    @State private var truncated = false
    @State private var total: Int64 = 0
    @State private var units: [Int64: String] = [:]
    @State private var tags: [Tag] = []
    @State private var problem: Problem?
    @State private var loaded = false
    /// Rows waiting for the server. Kept so a tap looks instant on a wrist, where the
    /// round trip is the phone's connection plus the server's.
    @State private var inFlight: Set<Int64> = []
    /// Ticks made here that have not been sent — see the phone's `ItemsView`.
    @State private var unsent: Set<String> = []
    @State private var waiting = 0
    @State private var fresh = false
    @State private var draining = false

    private let cache = Cache.shared

    private var outstanding: [Item] { items.filter { !$0.isDone } }

    /// Outstanding items in the order the shop is laid out, with no headings.
    ///
    /// The same grouping the phone and the browser use, flattened. Tags earn their
    /// place here by putting the right things next to each other; a heading over each
    /// run would cost a row apiece to say what the order already says, on a screen
    /// with six of them.
    private var ordered: [Item] { grouped(outstanding, by: tags).flatMap(\.items) }
    private var done: [Item] { items.filter(\.isDone) }

    var body: some View {
        Group {
            if !loaded {
                ProgressView()
            } else if let problem, items.isEmpty {
                // Only when there is nothing to show. A failed request used to replace
                // the list with this, which threw away the one thing the person came to
                // look at over a connection they cannot do anything about.
                WatchProblemView(problem: problem) { Task { await load() } }
            } else {
                SwiftUI.List {
                    if outstanding.isEmpty && fresh {
                        Text(items.isEmpty ? "Nothing on this list." : "All done.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }

                    ForEach(ordered) { row($0) }

                    // Short, because there is no room for more -- but said, because
                    // a wrist showing a prefix of a list is the worst place to
                    // discover that quietly.
                    if truncated {
                        Text("\(ordered.count) of \(total) shown")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }

                    if !done.isEmpty {
                        Section {
                            ForEach(done) { row($0) }
                        } header: {
                            Text("\(done.count) done")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .textCase(nil)
                        }
                    }
                }
                // Plain rather than the default: the inset style spends a margin on
                // each side of every row, and on 208 points of width that margin is
                // the difference between four rows and six.
                .listStyle(.plain)
                .environment(\.defaultMinListRowHeight, Self.rowHeight)
            }
        }
        .navigationTitle(list.name)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                WatchStatusDot(waiting: waiting, offline: problem != nil)
            }
        }
        .task { await loadReference() }
        .task {
            showWhatWeHave()
            refreshUnsent()
            await load()
        }
        .task { await watch() }
        .task { await keepTrying() }
        // watchOS suspends a connection the moment the app stops being frontmost, so
        // coming back is the gap the stream cannot cover on its own.
        .onChange(of: phase) { _, now in
            if now == .active { Task { await load() } }
        }
    }

    /// Keeps the wrist in step with the phone and the browser.
    ///
    /// The same stream the phone watches, through the same shared client. A watch is
    /// the screen most likely to be showing a list somebody else is changing -- the
    /// other half of the shop, holding the phone -- so it is the one that can least
    /// afford to be quietly stale.
    private func watch() async {
        var reconnecting = false

        while !Task.isCancelled {
            if reconnecting { await load() }

            do {
                for try await _ in try await api.changes(on: list) {
                    await load()
                }
            } catch let problem as APIError {
                // A stream refused for want of a token means the cached one has
                // expired. Dropping it makes the next attempt ask the phone again,
                // which is the whole recovery path on a watch.
                if case .unauthorized = problem {
                    identity.refused()
                }
            } catch {}

            // A watch loses its connection constantly -- a lowered wrist is enough --
            // so this is ordinary rather than an error worth showing.
            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
        }
    }

    /// A row says what it says on the phone: struck through, greyed, and under the
    /// done heading. Three signals, none of them a control -- a box you can tick is
    /// the row's job, and drawing one only repeats what tapping already does.
    ///
    /// The dimming while a tap is in flight is not decoration here. It is the only
    /// thing between the tap and the server's answer, and on a wrist that gap is the
    /// phone's connection plus the server's.
    /// How tall a row is allowed to be.
    ///
    /// Chosen against the screen rather than by feel: a 46mm watch leaves about 204
    /// points below the title, so 34 fits six rows and 44 fits four. It does not go
    /// lower than this, and the reason is particular to this screen -- the row *is*
    /// the tap target, tapping is the only thing this app does, and a mis-tap crosses
    /// off the wrong item, which you discover at home. A seventh row is not worth
    /// that.
    private static let rowHeight: CGFloat = 34

    private func row(_ item: Item) -> some View {
        Button {
            Task { await toggle(item) }
        } label: {
            HStack(spacing: 6) {
                Text(item.name)
                    .font(.footnote)
                    // Two lines rather than one: a truncated name on a shopping list
                    // is the one thing the row exists to tell you.
                    .lineLimit(2)
                    .strikethrough(item.isDone)
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                Spacer(minLength: 2)

                // Quietly, in the row rather than over it: a tick that has not been
                // sent is a detail about that line, not news about the app. Laid out
                // beside the measure rather than on top of it -- a wrist has no room
                // to spare and an overlay simply sat on the number.
                if unsent.contains(item.uuid) {
                    Image(systemName: "clock")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .layoutPriority(1)
                }

                if let measure = item.measure(units: units) {
                    Text(measure)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                        // Never squeezed to make room for a long name: the amount is
                        // short and the name is the part that can afford to wrap.
                        .layoutPriority(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .opacity(inFlight.contains(item.id) ? 0.4 : 1)
        }
        .buttonStyle(.plain)
        .disabled(inFlight.contains(item.id) || !list.mayEdit)
        .listRowInsets(EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8))
    }

    private func toggle(_ item: Item) async {
        // A viewer's tap would be refused by the server, and a row that greys out and
        // comes back unchanged is a worse answer than one that does not move.
        guard list.mayEdit else { return }

        let done = !item.isDone
        cache.outbox.setDone(item, on: list, done: done)
        items = items.map { $0.uuid == item.uuid ? $0.withDone(done) : $0 }
        cache.remember(items: items, on: list)
        refreshUnsent()

        await drain()
    }

    /// Sends what is queued. See the phone's `ItemsView.drain` — the rules are the
    /// same, and only the losses are said out loud.
    private func drain() async {
        guard !draining else { return }
        // See the phone's copy: the lists screen drains the same queue, so this count
        // goes stale unless it is read back even when there is nothing to send.
        refreshUnsent()
        guard cache.outbox.waiting > 0 else { return }

        draining = true
        let drained = await cache.outbox.drain(through: api)
        draining = false

        refreshUnsent()
        if drained.sent > 0 { await load() }
    }

    /// Tries the queue again while anything is in it, so a tick does not wait for
    /// somebody else to touch the list.
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

    /// The last list this watch saw, put up before anything is asked of the server.
    private func showWhatWeHave() {
        guard !fresh else { return }
        let remembered = cache.items(on: list)
        guard !remembered.isEmpty else { return }
        items = remembered
        total = Int64(remembered.count)
        loaded = true
    }

    /// The server's answer with this watch's unsent ticks laid back over it.
    private func withUnsent(_ fromServer: [Item]) -> [Item] {
        let queued = cache.outbox.forList(list)
        guard !queued.isEmpty else { return fromServer }
        var rows = fromServer
        for operation in queued where operation.kind == QueuedOperation.Kind.setDone {
            rows = rows.map {
                $0.uuid == operation.itemUUID ? $0.withDone(operation.done) : $0
            }
        }
        return rows
    }

    /// Reference data, once. On a watch this matters twice over: it is the slowest
    /// connection of the three, and it is relayed through a phone.
    private func loadReference() async {
        do {
            async let units = api.units()
            async let tags = api.tags(orderedFor: list)
            let (loadedUnits, loadedTags) = try await (units, tags)
            self.units = Dictionary(uniqueKeysWithValues: loadedUnits.map { ($0.id, $0.name) })
            self.tags = loadedTags
        } catch {}
    }

    private func load() async {
        do {
            let listing = try await api.items(on: list)
            cache.remember(items: listing.items, on: list)
            self.items = withUnsent(listing.items)
            self.truncated = listing.truncated
            self.total = listing.total
            problem = nil
            fresh = true
            loaded = true
            await drain()
        } catch {
            problem = Problem(error, identity: identity)
        }
        loaded = true
    }
}
