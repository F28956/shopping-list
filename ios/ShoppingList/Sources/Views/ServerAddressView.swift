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

    /// What to do when somebody says they have no server. `nil` hides the offer, which
    /// is right when this screen is reached from settings — a device that already has
    /// lists on a server is not choosing for the first time.
    var declined: (() -> Void)?

    @State private var suggestion: ServerAddress?

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

            if let suggestion {
                // Shown, not silently adopted. The host is the thing being trusted, so
                // it is the thing on screen.
                Button {
                    typed = suggestion.origin
                    Task { await check() }
                } label: {
                    VStack(spacing: 2) {
                        Text("Use \(suggestion.origin)")
                        Text("from the link you copied").font(.caption)
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

            // C7. Reading the pasteboard is an explicit tap rather than something
            // that happens on appear: a silent read shows the system's paste banner
            // and rummages through somebody's clipboard uninvited.
            Button("I have a share link", systemImage: "link") { readTheClipboard() }
                .font(.footnote)
                .accessibilityIdentifier("paste-share-link")

            if let declined {
                // S1. The app has to be useful before it has a server, and this is
                // where somebody says they do not want one. It is not a lesser mode:
                // lists made here work exactly as lists made with no signal do, and
                // attaching a server later sends them.
                VStack(spacing: 4) {
                    Button("Use this device only", action: declined)
                        .accessibilityIdentifier("no-server")
                    Text("Your lists stay on this phone. You can add a server later.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                .padding(.top, 8)
            }

            if let problem {
                Text(problem)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(32)
        .onAppear { editing = true }
    }

    /// Takes the origin out of a share link somebody copied.
    ///
    /// This is how C7 has to work, and the reason is worth writing down: a share link
    /// **cannot** open this app directly. Universal links match an associated domain
    /// baked into the app at build time, and every self-hoster's domain is different —
    /// so there is no domain to associate. The clipboard is the only route a link has.
    @MainActor
    private func readTheClipboard() {
        guard let pasted = UIPasteboard.general.string else {
            problem = "There is no link on the clipboard."
            return
        }

        guard let found = server(in: pasted) else {
            problem = "That does not look like a share link."
            return
        }

        problem = nil
        suggestion = found
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
