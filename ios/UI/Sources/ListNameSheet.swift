import SwiftUI

/// Naming a list, whether it is a new one or one being renamed.
///
/// Shared by the phone and the Mac because there is nothing platform-shaped about a
/// field and two buttons, and because the rule about what counts as a name is the
/// thing worth having in one place.
struct ListNameSheet: View {
    enum Purpose: Identifiable {
        case create
        case rename(List)

        /// Distinct per list, so reopening on another one rebuilds the sheet rather
        /// than reusing the field with the previous name still in it.
        var id: String {
            switch self {
            case .create: return "create"
            case .rename(let list): return "rename-\(list.id)"
            }
        }

        var title: String {
            switch self {
            case .create: return "New list"
            case .rename: return "Rename list"
            }
        }

        var confirm: String {
            switch self {
            case .create: return "Create"
            case .rename: return "Rename"
            }
        }
    }

    let purpose: Purpose
    let save: (String) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @FocusState private var naming: Bool

    init(purpose: Purpose, save: @escaping (String) async -> Void) {
        self.purpose = purpose
        self.save = save
        switch purpose {
        case .create: _name = State(initialValue: "")
        case .rename(let list): _name = State(initialValue: list.name)
        }
    }

    /// What would be sent, or nil when it is not a name. The server trims and refuses
    /// an empty one; refusing here means the button does not offer to fail.
    private var typed: String? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(purpose.title)
                .font(.headline)
                .accessibilityIdentifier("listname.title")

            TextField("Name", text: $name)
                .textFieldStyle(.roundedBorder)
                .focused($naming)
                .accessibilityIdentifier("listname.field")
                .onSubmit { commit() }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("listname.cancel")
                Button(purpose.confirm) { commit() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(typed == nil)
                    .accessibilityIdentifier("listname.confirm")
            }
        }
        .padding(20)
        .frame(minWidth: 320)
        .task { naming = true }
    }

    private func commit() {
        guard let typed else { return }
        dismiss()
        Task { await save(typed) }
    }
}
