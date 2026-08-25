import SwiftUI

/// What is left to get, and one gesture: tap to cross off, tap again to put back.
///
/// There is no adding, no editing, no deleting and no tags. Not because a watch could
/// not show them, but because the moment this screen is for is one hand on a trolley
/// — and a row that does two things is a row that does the wrong one.
struct WatchItemsView: View {
    let api: API
    let list: List
    @Environment(WatchIdentity.self) private var identity
    @Environment(\.scenePhase) private var phase

    @State private var items: [Item] = []
    @State private var units: [Int64: String] = [:]
    @State private var problem: Problem?
    @State private var loaded = false
    /// Rows waiting for the server. Kept so a tap looks instant on a wrist, where the
    /// round trip is the phone's connection plus the server's.
    @State private var inFlight: Set<Int64> = []

    private var outstanding: [Item] { items.filter { !$0.isDone } }
    private var done: [Item] { items.filter(\.isDone) }

    var body: some View {
        Group {
            if !loaded {
                ProgressView()
            } else if let problem {
                WatchProblemView(problem: problem) { Task { await load() } }
            } else {
                SwiftUI.List {
                    if outstanding.isEmpty {
                        Text(items.isEmpty ? "Nothing on this list." : "All done.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }

                    ForEach(outstanding) { row($0) }

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
        .task { await load() }
        .task { await watch() }
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
        .disabled(inFlight.contains(item.id))
        .listRowInsets(EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8))
    }

    private func toggle(_ item: Item) async {
        inFlight.insert(item.id)
        defer { inFlight.remove(item.id) }

        do {
            try await api.setDone(item, on: list, done: !item.isDone)
            await load()
        } catch {
            problem = Problem(error, identity: identity)
        }
    }

    private func load() async {
        do {
            async let items = api.items(on: list)
            async let units = api.units()
            let (loadedItems, loadedUnits) = try await (items, units)
            self.items = loadedItems
            self.units = Dictionary(uniqueKeysWithValues: loadedUnits.map { ($0.id, $0.name) })
            problem = nil
        } catch {
            problem = Problem(error, identity: identity)
        }
        loaded = true
    }
}
