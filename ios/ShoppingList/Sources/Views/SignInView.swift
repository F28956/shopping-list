import SwiftUI

struct SignInView: View {
    @Environment(Identity.self) private var identity

    var body: some View {
        VStack(spacing: 20) {
            Text("Shopping list")
                .font(.largeTitle.weight(.semibold))
            Text("Your lists, on the phone you take to the shop.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            if identity.isConfigured {
                Button("Sign in with Google") {
                    Task { await signIn() }
                }
                .buttonStyle(.borderedProminent)
            } else {
                // Said plainly rather than failing at the tap: a fresh clone has no
                // client id, because the file holding it is not committed.
                Text("This build has no Google client id yet.\nSee ios/README.md.")
                    .font(.footnote)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
            }

            if let error = identity.lastError {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(32)
    }

    @MainActor
    private func signIn() async {
        guard let root = UIApplication.shared.rootViewController else { return }
        await identity.signIn(presenting: root)
    }
}

extension UIApplication {
    /// The controller to present the sign-in sheet from.
    var rootViewController: UIViewController? {
        connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
            .first { $0.isKeyWindow }?
            .rootViewController
    }
}
