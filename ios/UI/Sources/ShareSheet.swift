import SwiftUI

/// Who can see this list, and how to let somebody else in.
///
/// A link rather than an address: the server knows people by their Google account and
/// has no way to look one up by email, so an invitation is something you send by
/// whatever you already use to talk to them.
struct ShareSheet: View {
    let list: List
    let api: API
    /// Called after anything changes, so the screen behind can catch up.
    let changed: () async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var people: [Person] = []
    @State private var me: Int64?
    @State private var link: URL?
    @State private var loaded = false
    @State private var error: String?
    @State private var leaving = false

    private var iOwnIt: Bool { list.role >= .owner }

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                if let link {
                    Section {
                        // Shown once. Only the hash is stored, so a link that is lost
                        // is remade rather than looked up.
                        Text(link.absoluteString)
                            .font(.footnote.monospaced())
                            .textSelection(.enabled)
                            .accessibilityIdentifier("share.link")

                        Button("Copy link") { copy(link.absoluteString) }
                            .accessibilityIdentifier("share.copy")
                    } header: {
                        Text("Send this to one person")
                    } footer: {
                        Text(
                            "It works once, for whoever opens it first, and expires "
                                + "in a week. It is shown only now."
                        )
                    }
                }

                Section("Who can see it") {
                    if !loaded {
                        ProgressView()
                    }
                    ForEach(people) { person in
                        HStack {
                            VStack(alignment: .leading, spacing: 1) {
                                Text(person.shown)
                                if person.name != nil, let email = person.email {
                                    Text(email)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            Spacer()
                            Text(person.userID == me ? "you" : person.role.rawValue)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .accessibilityIdentifier("share.person.\(person.userID)")
                        .swipeActions(edge: .trailing) {
                            // An owner may remove anybody but themselves; there is no
                            // transfer, so a list without its owner is a list nobody
                            // could rename or delete.
                            if iOwnIt, person.role < .owner {
                                Button(role: .destructive) {
                                    Task { await act { try await api.remove(person, from: list) } }
                                } label: {
                                    Label("Remove", systemImage: "person.badge.minus")
                                }
                            }
                        }
                    }
                }

                if iOwnIt {
                    Section {
                        Button("Create a link") {
                            Task { await act { link = try await api.invite(to: list) } }
                        }
                        .accessibilityIdentifier("share.invite")

                        Button("Withdraw all links", role: .destructive) {
                            Task {
                                await act { try await api.revokeInvites(to: list) }
                                link = nil
                            }
                        }
                        .accessibilityIdentifier("share.revoke")
                    } footer: {
                        Text(
                            "Withdrawing cancels every link not yet used. People "
                                + "already on the list stay."
                        )
                    }
                } else {
                    Section {
                        Button("Leave this list", role: .destructive) { leaving = true }
                            .accessibilityIdentifier("share.leave")
                    } footer: {
                        Text("You will need a new link to come back.")
                    }
                }
            }
            .navigationTitle("Share \(list.name)")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("share.done")
                }
            }
            .confirmationDialog(
                "Leave \(list.name)?",
                isPresented: $leaving,
                titleVisibility: .visible
            ) {
                Button("Leave", role: .destructive) {
                    Task {
                        // Dismissed first: the sheet is about a list this person is
                        // about to stop being able to see.
                        dismiss()
                        await act { try await api.remove(mine(), from: list) }
                    }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("It stays on the list for everyone else.")
            }
            .alert("Something went wrong", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
            .task { await load() }
        }
        .frame(minWidth: 380, minHeight: 420)
    }

    /// Me, as a `Person`, so leaving is removing like any other.
    private func mine() -> Person {
        people.first { $0.userID == me }
            ?? Person(userID: me ?? 0, name: nil, email: nil, role: list.role)
    }

    private func copy(_ text: String) {
        #if os(macOS)
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        #else
            UIPasteboard.general.string = text
        #endif
    }

    /// Runs something, reports what went wrong, and lets the screen behind catch up.
    private func act(_ work: () async throws -> Void) async {
        do {
            try await work()
            await load()
            await changed()
        } catch let problem as APIError {
            error = problem.localizedDescription
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func load() async {
        do {
            async let people = api.people(on: list)
            async let me = api.whoAmI()
            (self.people, self.me) = try await (people, me.id)
            error = nil
        } catch let problem as APIError {
            error = problem.localizedDescription
        } catch {
            self.error = error.localizedDescription
        }
        loaded = true
    }
}
