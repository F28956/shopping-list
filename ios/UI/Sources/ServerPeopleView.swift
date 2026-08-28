import SwiftUI

/// Who may use this server.
///
/// Reached only by an owner, and gated on `Me.is_owner` rather than on hope: every
/// route behind it is refused in `domain::service::admission` to anybody else, so
/// hiding the screen is a courtesy and not the check.
///
/// Worth saying on the screen and worth remembering here: **an owner is not a data
/// role.** They decide who may use the machine and have no more access to anybody's
/// lists than anybody else does.
struct ServerPeopleView: View {
    let api: API

    @Environment(\.dismiss) private var dismiss

    @State private var admitted: [Admitted] = []
    @State private var about: ServerAbout?
    @State private var loaded = false
    @State private var problem: String?
    @State private var admitting = false
    @State private var withdrawing: Admitted?

    var body: some View {
        NavigationStack {
            Group {
                if !loaded {
                    ProgressView()
                } else {
                    people
                }
            }
            .navigationTitle("Who may sign in")
            .compactTitle()
            // Adding on the left, finishing on the right -- the shape Settings >
            // Passwords uses, and the one a modal list with an add action wants. These
            // were the other way round once, which put `Done` in the slot that means
            // cancel. On a Mac they are a row along the bottom, because a sheet's
            // toolbar is not drawn there at all -- see `sheetActions`.
            .sheetActions {
                Button("Admit", systemImage: "plus") { admitting = true }
                    .accessibilityIdentifier("admit")
            } finishing: {
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .task { await load() }
            .sheet(isPresented: $admitting) {
                AdmitSheet { email, note in
                    await attempt { try await api.admit(email, note: note) }
                }
                .presentationDetents([.height(260)])
            }
            .alert(item: $withdrawing) { row in
                Alert(
                    title: Text("Withdraw \(row.email)?"),
                    message: Text(
                        row.isInUse
                            ? "They are signed in. This takes effect on their very next request."
                            : "Nobody has used this address yet."
                    ),
                    primaryButton: .destructive(Text("Withdraw")) {
                        Task { await attempt { try await api.withdraw(row.email) } }
                    },
                    secondaryButton: .cancel()
                )
            }
        }
        .sheetSize()
    }

    private var people: some View {
        // `SwiftUI.List`, because this app has a `List` of its own and it is a
        // shopping list.
        SwiftUI.List {
            if let problem {
                Section { Text(problem).foregroundStyle(.red).font(.footnote) }
            }

            Section {
                ForEach(admitted) { row in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.note ?? row.email)
                        if row.note != nil {
                            Text(row.email).font(.caption).foregroundStyle(.secondary)
                        }
                        Text(row.isInUse ? "Signed in here" : "Has not signed in yet")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    .swipeActions {
                        Button("Withdraw", role: .destructive) { withdrawing = row }
                    }
                    .contextMenu {
                        // Also here, and not only behind the swipe above: a swipe is an
                        // iOS gesture, so on a Mac this was the one screen where
                        // somebody could be admitted and never withdrawn.
                        Button("Withdraw", role: .destructive) { withdrawing = row }
                        Divider()
                        // Only somebody who has been here can be made an owner: there
                        // is no person yet to make one of, and the server says so.
                        if row.isInUse {
                            Button("Make an owner", systemImage: "key") {
                                Task { await attempt { try await api.setOwner(row.email, true) } }
                            }
                            Button("Not an owner", systemImage: "key.slash") {
                                Task { await attempt { try await api.setOwner(row.email, false) } }
                            }
                        }
                        Button("Withdraw", systemImage: "trash", role: .destructive) {
                            withdrawing = row
                        }
                    }
                }
            } header: {
                Text("Admitted")
            } footer: {
                Text(
                    """
                    Being an owner means deciding who may sign in. It does not give \
                    anybody access to anybody else's lists.
                    """
                )
            }

            if let about {
                Section {
                    Toggle(
                        "Anyone may sign in",
                        isOn: Binding(
                            get: { about.admitsAnyone },
                            set: { open in
                                Task { await attempt { try await api.setAdmitsAnyone(open) } }
                            }
                        )
                    )
                    .accessibilityIdentifier("admits-anyone")
                } footer: {
                    Text(
                        about.admitsAnyone
                            ? "Anybody who can sign in with Apple or Google can use this server."
                            : "Only the addresses above can sign in."
                    )
                }
            }
        }
    }

    /// Runs something and reloads, so the screen shows what the server now thinks
    /// rather than what this device hoped.
    private func attempt(_ work: () async throws -> Void) async {
        do {
            try await work()
            problem = nil
        } catch {
            problem = (error as? APIError)?.errorDescription ?? error.localizedDescription
        }
        await load()
    }

    private func load() async {
        do {
            admitted = try await api.admissions()
            about = try await api.serverAbout()
        } catch {
            problem = (error as? APIError)?.errorDescription ?? error.localizedDescription
        }
        loaded = true
    }
}

/// Admitting one address.
private struct AdmitSheet: View {
    let admit: (String, String?) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var email = ""
    @State private var note = ""

    var body: some View {
        NavigationStack {
            Form {
                TextField("Their address", text: $email)
                    .textContentType(.emailAddress)
                    .emailEntry()
                    .autocorrectionDisabled()
                    .accessibilityIdentifier("admit.email")
                // "mum", so that a list of addresses stays readable.
                TextField("A name for them, optionally", text: $note)
            }
            .navigationTitle("Admit somebody")
            .compactTitle()
            .sheetActions {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
            } confirming: {
                Button("Admit") {
                    let address = email.trimmingCharacters(in: .whitespaces)
                    let label = note.trimmingCharacters(in: .whitespaces)
                    dismiss()
                    Task { await admit(address, label.isEmpty ? nil : label) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(email.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .sheetSize()
    }
}
