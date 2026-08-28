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
    let list: List
    @Environment(WatchStore.self) private var store
    @Environment(\.scenePhase) private var phase

    /// Everything about the list itself -- see `WatchItemsModel`.
    @State private var model: WatchItemsModel

    init(list: List, store: WatchStore) {
        self.list = list
        _model = State(initialValue: WatchItemsModel(list: list, store: store))
    }

    var body: some View {
        Group {
            if !model.loaded {
                ProgressView()
            } else if let problem = model.problem, model.items.isEmpty {
                // Only when there is nothing to show. A failed request used to replace
                // the list with this, which threw away the one thing the person came to
                // look at over a connection they cannot do anything about.
                WatchProblemView(problem: problem) { Task { await model.load() } }
            } else {
                SwiftUI.List {
                    if model.outstanding.isEmpty && model.fresh {
                        Text(model.items.isEmpty ? "Nothing on this list." : "All done.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }

                    ForEach(model.ordered) { row($0) }

                    // Short, because there is no room for more -- but said, because
                    // a wrist showing a prefix of a list is the worst place to
                    // discover that quietly.
                    if model.truncated {
                        Text("\(model.ordered.count) of \(model.total) shown")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }

                    if !model.done.isEmpty {
                        Section {
                            ForEach(model.done) { row($0) }
                        } header: {
                            Text("\(model.done.count) done")
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
                WatchStatusDot(waiting: model.waiting, offline: model.problem != nil)
            }
        }
        .task { await model.loadReference() }
        .task {
            model.showWhatWeHave()
            model.refreshUnsent()
            await model.load()
        }
        .task { await model.watch() }
        .task { await model.keepTrying() }
        // watchOS suspends a connection the moment the app stops being frontmost, so
        // coming back is the gap the stream cannot cover on its own.
        .onChange(of: phase) { _, now in
            if now == .active { Task { await model.load() } }
        }
        // With no server this is how a change arrives: the phone pushes a picture and
        // `WatchStore` writes it into the cache.
        .onReceive(NotificationCenter.default.publisher(for: .cacheChanged)) { _ in
            model.cacheChanged()
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
            Task { await model.toggle(item) }
        } label: {
            HStack(spacing: 6) {
                Text(item.name)
                    .font(.footnote)
                    // Two lines rather than one: a truncated name on a shopping list
                    // is the one thing the row exists to tell you.
                    .lineLimit(2)
                    .strikethrough(item.isDone)
                    .foregroundStyle(item.isDone ? .secondary : .primary)
                // The tag that put it here, and only that one — on the same line, as
                // one glyph. A wrist sorted by these already says where things are;
                // the mark is a reminder of which run you are in, and every tag would
                // be a second column on a screen that has no room for a first.
                if let primary = primaryTag(of: item, in: model.tags) {
                    Text(primary.mark)
                        .font(.caption2)
                        .accessibilityHidden(true)
                }

                Spacer(minLength: 2)

                // Quietly, in the row rather than over it: a tick that has not been
                // sent is a detail about that line, not news about the app. Laid out
                // beside the measure rather than on top of it -- a wrist has no room
                // to spare and an overlay simply sat on the number.
                if model.unsent.contains(item.uuid) {
                    Image(systemName: "clock")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .layoutPriority(1)
                }

                if let measure = item.measure(units: model.units) {
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
            .opacity(model.inFlight.contains(item.id) ? 0.4 : 1)
        }
        .buttonStyle(.plain)
        .disabled(model.inFlight.contains(item.id) || !list.mayEdit)
        .listRowInsets(EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8))
    }
}
