import SwiftUI

/// Which tag decides where an item sits on this list.
///
/// Drag to reorder. What you move to the top leads; everything you leave alone keeps
/// the order a shop is walked in, behind it. Yours alone — two people sharing a list
/// are not necessarily walking the same route — and a list where nobody has chosen
/// takes whichever order was set here first.
struct TagOrderSheet: View {
    let list: List
    /// As the server resolved it: the order in force right now.
    let tags: [Tag]
    let save: ([Tag]) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var ordered: [Tag]

    init(list: List, tags: [Tag], save: @escaping ([Tag]) async -> Void) {
        self.list = list
        self.tags = tags
        self.save = save
        _ordered = State(initialValue: tags)
    }

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                Section {
                    ForEach(ordered) { tag in
                        HStack(spacing: 8) {
                            if let emoji = tag.emoji, !emoji.isEmpty { Text(emoji) }
                            Text(tag.name)
                        }
                        .accessibilityIdentifier("order.\(tag.name)")
                    }
                    .onMove { from, to in ordered.move(fromOffsets: from, toOffset: to) }
                } footer: {
                    Text(
                        "Items sit under the first of their tags in this order. "
                            + "This is your order for \(list.name); everyone else keeps theirs."
                    )
                }
            }
            #if os(iOS)
                .environment(\.editMode, .constant(.active))
            #endif
            .navigationTitle("Tag order")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("order.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        let chosen = ordered
                        dismiss()
                        Task { await save(chosen) }
                    }
                    .accessibilityIdentifier("order.save")
                }
                ToolbarItem(placement: .destructiveAction) {
                    // Back to the shop's own order, which is also what a list nobody
                    // has touched already does.
                    Button("Reset") {
                        dismiss()
                        Task { await save([]) }
                    }
                    .accessibilityIdentifier("order.reset")
                }
            }
        }
        .frame(minWidth: 320, minHeight: 380)
    }
}
