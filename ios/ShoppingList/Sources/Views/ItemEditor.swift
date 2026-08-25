import SwiftUI

/// Correcting a row: what it is, how much, and in what.
///
/// A sheet rather than a field that opens inside the row. On a phone the keyboard
/// covers half the screen, and a row that grows under your thumb while the list
/// reflows around it is how you tap the wrong thing.
struct ItemEditor: View {
    let units: [Unit]
    let tags: [Tag]
    /// Hands back what was typed. The caller talks to the API, because by the time
    /// a request fails this sheet is closed and the list is the thing that has to
    /// say so.
    let save: (ItemEdit) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var draft: ItemDraft
    @FocusState private var naming: Bool

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
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $draft.name)
                        .focused($naming)
                        .autocorrectionDisabled()
                        .submitLabel(.done)
                }

                Section("How much") {
                    TextField("Amount", text: $draft.amount)
                        .keyboardType(.decimalPad)
                    Picker("Unit", selection: $draft.unitID) {
                        // Most things are counted rather than measured, so no unit is
                        // an ordinary answer and belongs at the top rather than hidden
                        // at the bottom of the list.
                        Text("None").tag(Int64?.none)
                        ForEach(units) { unit in
                            Text(unit.name).tag(Int64?.some(unit.id))
                        }
                    }
                }

                if !tags.isEmpty {
                    Section("Where it lives") {
                        ForEach(tags) { tag in
                            Button {
                                // A set, so tapping the one already on is how it comes
                                // off -- there is no separate remove to go looking for.
                                if draft.tagIDs.contains(tag.id) {
                                    draft.tagIDs.remove(tag.id)
                                } else {
                                    draft.tagIDs.insert(tag.id)
                                }
                            } label: {
                                HStack {
                                    if let emoji = tag.emoji, !emoji.isEmpty {
                                        Text(emoji)
                                    }
                                    Text(tag.name)
                                    Spacer()
                                    if draft.tagIDs.contains(tag.id) {
                                        Image(systemName: "checkmark")
                                            .foregroundStyle(Color.accentColor)
                                    }
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            .navigationTitle("Edit item")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        guard let edit = draft.validated else { return }
                        // Closed first: the sheet has nothing left to say, and leaving
                        // it up during the round trip only invites a second tap.
                        dismiss()
                        Task { await save(edit) }
                    }
                    .disabled(draft.validated == nil)
                }
            }
            // Opened to correct a name far more often than an amount, and one tap
            // saved is the whole point of the gesture that got here.
            .task { naming = true }
        }
    }
}
