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
    /// The tags actually on this list's items.
    ///
    /// Shown because the order only bites through the tags an item carries, and a
    /// screen of twenty-one names gives no clue which those are — `bakery` and
    /// `baking` sit eight rows apart and read the same at a glance. Moving the one
    /// nothing is filed under looks exactly like the feature not working.
    let inUse: Set<Int64>
    let save: ([Tag]) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var ordered: [Tag]

    init(
        list: List,
        tags: [Tag],
        inUse: Set<Int64>,
        save: @escaping ([Tag]) async -> Void
    ) {
        self.list = list
        self.tags = tags
        self.inUse = inUse
        self.save = save
        _ordered = State(initialValue: tags)
    }

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                Section {
                    ForEach(ordered) { tag in
                        let used = inUse.contains(tag.id)

                        HStack(spacing: 8) {
                            if let emoji = tag.emoji, !emoji.isEmpty { Text(emoji) }
                            Text(tag.name)
                                .foregroundStyle(used ? .primary : .secondary)
                            if used {
                                Text("on this list")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .padding(.horizontal, 5)
                                    .padding(.vertical, 1)
                                    .background(.quaternary, in: Capsule())
                            }
                        }
                        // One element, not three. The row is a name, maybe an emoji
                        // and maybe a chip; read separately those arrive as loose
                        // words, and queried separately they match more than once.
                        .accessibilityElement(children: .ignore)
                        .accessibilityIdentifier("order.\(tag.name)")
                        .accessibilityLabel(
                            used ? "\(tag.name), on this list" : tag.name
                        )
                    }
                    .onMove { from, to in ordered.move(fromOffsets: from, toOffset: to) }
                } footer: {
                    Text(
                        "Items sit under the first of their tags in this order. "
                            + "Moving a tag nothing is filed under changes nothing. "
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
