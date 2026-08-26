import Foundation

/// A saveable edit: what the fields came to mean.
struct ItemEdit: Equatable {
    var name: String
    var amount: Double
    var unitID: Int64?
    var tagIDs: Set<Int64>
}

/// What the item editor has been typed into, and whether it can be saved.
///
/// Separate from the view because this is the part with rules in it. A view is
/// awkward to test and this is the half that decides whether the phone sends the
/// server something it will refuse.
struct ItemDraft: Equatable {
    var name: String
    var amount: String
    var unitID: Int64?
    /// Tags are attached and detached by their own routes, not by the update. They
    /// are held here anyway so that Cancel undoes them along with everything else:
    /// applying them as they are tapped would make one control on the sheet behave
    /// differently from the rest of it.
    var tagIDs: Set<Int64>

    init(item: Item, tags: [Tag] = []) {
        name = item.name
        amount = item.amount.asAmount
        unitID = item.unitID
        tagIDs = Set(tags.map(\.id))
    }

    /// The values to send, or nil when what is typed is not a saveable item.
    ///
    /// nil is also what greys out Save, so there is one rule rather than two that can
    /// drift apart — the button cannot offer to send something the server refuses.
    var validated: ItemEdit? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        // A comma is the decimal separator across most of Europe and the decimal pad
        // offers whichever the phone is set to, so both have to be read. `Double(_:)`
        // only accepts a full stop.
        let typed = amount
            .trimmingCharacters(in: .whitespaces)
            .replacingOccurrences(of: ",", with: ".")

        guard !trimmed.isEmpty, let quantity = Double(typed), quantity > 0, quantity.isFinite
        else { return nil }

        return ItemEdit(name: trimmed, amount: quantity, unitID: unitID, tagIDs: tagIDs)
    }
}
