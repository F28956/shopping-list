import SwiftUI

/// Correcting a row.
///
/// The same fields and the same rules as the phone's — `ItemDraft` decides what may
/// be saved for both — laid out for a pointer instead of a thumb.
struct MacItemEditor: View {
    let units: [Unit]
    let tags: [Tag]
    let save: (ItemEdit) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var draft: ItemDraft

    init(
        item: Item,
        units: [Unit],
        tags: [Tag],
        attached: [Tag],
        save: @escaping (ItemEdit) async -> Void
    ) {
        self.units = units
        self.tags = tags
        self.save = save
        _draft = State(initialValue: ItemDraft(item: item, tags: attached))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Edit item").font(.headline)

            Form {
                TextField("Name", text: $draft.name)
                TextField("Amount", text: $draft.amount)
                Picker("Unit", selection: $draft.unitID) {
                    Text("None").tag(Int64?.none)
                    ForEach(units) { unit in
                        Text(unit.name).tag(Int64?.some(unit.id))
                    }
                }
            }
            .formStyle(.grouped)

            if !tags.isEmpty {
                Text("Where it lives").font(.subheadline)
                // A wrapping grid rather than a list: twenty-one tags down a column
                // is a scroll on a screen with room to show them all at once.
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 130), alignment: .leading)],
                        alignment: .leading,
                        spacing: 6
                    ) {
                        ForEach(tags) { tag in
                            Toggle(isOn: binding(for: tag)) {
                                if let emoji = tag.emoji, !emoji.isEmpty {
                                    Text("\(emoji) \(tag.name)")
                                } else {
                                    Text(tag.name)
                                }
                            }
                            .toggleStyle(.checkbox)
                        }
                    }
                }
                .frame(maxHeight: 160)
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save") {
                    guard let edit = draft.validated else { return }
                    dismiss()
                    Task { await save(edit) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(draft.validated == nil)
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    /// Held in the draft rather than applied as it is ticked, so Cancel undoes tags
    /// along with everything else.
    private func binding(for tag: Tag) -> Binding<Bool> {
        Binding(
            get: { draft.tagIDs.contains(tag.id) },
            set: { on in
                if on {
                    draft.tagIDs.insert(tag.id)
                } else {
                    draft.tagIDs.remove(tag.id)
                }
            }
        )
    }
}
