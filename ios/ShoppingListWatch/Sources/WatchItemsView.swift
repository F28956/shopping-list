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
                        Section("\(done.count) done") {
                            ForEach(done) { row($0) }
                        }
                    }
                }
            }
        }
        .navigationTitle(list.name)
        .task { await load() }
    }

    /// A row says what it says on the phone: struck through, greyed, and under the
    /// done heading. Three signals, none of them a control -- a box you can tick is
    /// the row's job, and drawing one only repeats what tapping already does.
    ///
    /// The dimming while a tap is in flight is not decoration here. It is the only
    /// thing between the tap and the server's answer, and on a wrist that gap is the
    /// phone's connection plus the server's.
    private func row(_ item: Item) -> some View {
        Button {
            Task { await toggle(item) }
        } label: {
            HStack(spacing: 8) {
                Text(item.name)
                    .strikethrough(item.isDone)
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                Spacer(minLength: 4)
                if let measure = item.measure(units: units) {
                    Text(measure)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
            .contentShape(Rectangle())
            .opacity(inFlight.contains(item.id) ? 0.4 : 1)
        }
        .buttonStyle(.plain)
        .disabled(inFlight.contains(item.id))
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
