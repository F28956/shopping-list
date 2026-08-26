import SwiftUI

/// Accepting a code somebody sent you.
///
/// A pasted link works too — the browser still makes those — because both mean the
/// same request, and asking somebody to trim one themselves is asking them to do the
/// computer's job.
struct JoinSheet: View {
    let join: (String) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var pasted = ""
    @FocusState private var typing: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Join a list")
                .font(.headline)
                .accessibilityIdentifier("join.title")
            Text("Paste the code somebody sent you.")
                .font(.footnote)
                .foregroundStyle(.secondary)

            TextField("Code", text: $pasted)
                .textFieldStyle(.roundedBorder)
                .focused($typing)
                .accessibilityIdentifier("join.field")
                .onSubmit { commit() }
                #if os(iOS)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                #endif

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("join.cancel")
                Button("Join") { commit() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(token(in: pasted) == nil)
                    .accessibilityIdentifier("join.confirm")
            }
        }
        .padding(20)
        .frame(minWidth: 360)
        .task { typing = true }
    }

    private func commit() {
        guard let found = token(in: pasted) else { return }
        dismiss()
        Task { await join(found) }
    }
}
