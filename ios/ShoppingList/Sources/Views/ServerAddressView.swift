import SwiftUI

/// The first screen of a fresh install: which server.
///
/// C1 puts it before sign-in and never after. Signing in produces a token for a
/// particular audience and then sends it somewhere; there is no sensible order in
/// which the app authenticates first and discovers the destination second. The most
/// common refusal a new person meets — not being admitted — is also an answer only a
/// server can give.
struct ServerAddressView: View {
    /// What to do once an address has been accepted. The caller decides, because on a
    /// fresh install this leads to signing in and from settings it leads to throwing
    /// everything local away.
    let accepted: (ServerAddress, ServerDirectory.About) -> Void

    /// Offered rather than assumed, when a share link named its own origin (C7). A
    /// link is a bearer credential from an untrusted sender, and pointing an app at a
    /// host because a message said so is not something to do without showing the host.
    var suggestion: ServerAddress?

    @State private var typed = ""
    @State private var asking = false
    @State private var problem: String?
    @FocusState private var editing: Bool

    var body: some View {
        VStack(spacing: 20) {
            Text("Your server")
                .font(.largeTitle.weight(.semibold))
            Text("This app talks to a Shopping List server that you run.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            if let suggestion, typed.isEmpty {
                // Shown, not silently adopted. The host is the thing being trusted, so
                // it is the thing on screen.
                Button {
                    typed = suggestion.origin
                    Task { await check() }
                } label: {
                    VStack(spacing: 2) {
                        Text("Use \(suggestion.origin)")
                        Text("from the link you opened").font(.caption)
                    }
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("use-suggested-server")
            }

            TextField("shopping.example.com", text: $typed)
                .textFieldStyle(.roundedBorder)
                .textContentType(.URL)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.go)
                .focused($editing)
                .onSubmit { Task { await check() } }
                .accessibilityIdentifier("server-address")

            Button(asking ? "Checking…" : "Continue") {
                Task { await check() }
            }
            .buttonStyle(.borderedProminent)
            .disabled(asking || typed.trimmingCharacters(in: .whitespaces).isEmpty)

            if let problem {
                Text(problem)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(32)
        .onAppear { editing = suggestion == nil }
    }

    /// Parses, then asks. Both can fail and they fail differently, so the sentence on
    /// screen comes from whichever said no.
    @MainActor
    private func check() async {
        guard !asking else { return }

        let address: ServerAddress
        switch ServerAddress.parse(typed) {
        case .success(let parsed):
            address = parsed
        case .failure(let bad):
            problem = bad.sentence
            return
        }

        asking = true
        problem = nil
        defer { asking = false }

        switch await ServerDirectory.ask(address) {
        case .success(let about):
            // Shown back, because the repair is silent otherwise: somebody who typed a
            // host with no scheme should see what it became.
            typed = address.origin
            accepted(address, about)
        case .failure(let refusal):
            problem = refusal.sentence
        }
    }
}
