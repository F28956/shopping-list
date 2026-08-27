import SwiftUI

/// What is left to get, and one gesture: tap to cross off, tap again to put back.
///
/// There is no adding, no editing, no deleting and no tags. Not because a watch could
/// not show them, but because the moment this screen is for is one hand on a trolley —
/// and a row that does two things is a row that does the wrong one.
///
/// The one thing it *does* do, it does with the phone in a locker. A tick is a decision
/// already taken; it shows here immediately and reaches the phone whenever the two are
/// next in range — see `WatchLink`.
struct WatchItemsView: View {
    /// The list as it was when this screen opened. What is actually drawn is the store's
    /// copy of it, so a snapshot arriving while somebody is looking changes the screen
    /// rather than being ignored until they back out and come in again.
    let list: WatchLink.ListOnTheWatch
    @Environment(WatchStore.self) private var store

    /// The live version, or the one we opened with if the phone has since dropped it.
    private var current: WatchLink.ListOnTheWatch {
        store.lists.first { $0.id == list.id } ?? list
    }

    /// Already in the order the shop is walked — the phone grouped it before sending,
    /// so the rule lives in one place and this screen does not own a copy of it.
    private var outstanding: [WatchLink.ItemOnTheWatch] { current.items.filter { !$0.done } }
    private var done: [WatchLink.ItemOnTheWatch] { current.items.filter(\.done) }

    var body: some View {
        SwiftUI.List {
            if outstanding.isEmpty {
                Text(current.items.isEmpty ? "Nothing on this list." : "All done.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            ForEach(outstanding) { row($0) }

            // Short, because there is no room for more -- but said, because a wrist
            // showing a prefix of a list is the worst place to discover that quietly.
            if current.truncated {
                Text("\(current.items.count) of \(current.total) shown")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            if !done.isEmpty {
                Section {
                    ForEach(done) { row($0) }
                } header: {
                    Text("Done")
                }
            }
        }
        .navigationTitle(current.name)
    }

    private func row(_ item: WatchLink.ItemOnTheWatch) -> some View {
        Button {
            store.toggle(item, on: current)
        } label: {
            HStack {
                Image(systemName: item.done ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(item.done ? Color.accentColor : .secondary)

                Text(item.name)
                    .strikethrough(item.done)
                    .foregroundStyle(item.done ? .secondary : .primary)
                    .lineLimit(2)

                Spacer(minLength: 4)

                if let measure = item.measure {
                    Text(measure)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                        // Never squeezed: at an accessibility size a row can run out of
                        // width entirely, and a Text with no floor of its own wraps one
                        // letter per line down the side of the row.
                        .fixedSize(horizontal: true, vertical: false)
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(spoken(item))
        .accessibilityAddTraits(item.done ? [.isButton, .isSelected] : .isButton)
    }

    private func spoken(_ item: WatchLink.ItemOnTheWatch) -> String {
        let measure = item.measure.map { ", \($0)" } ?? ""
        let state = item.done ? ", crossed off" : ""
        return "\(item.name)\(measure)\(state)"
    }
}
