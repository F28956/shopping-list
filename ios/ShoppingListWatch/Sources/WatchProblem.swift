import SwiftUI

/// What went wrong, in terms a watch can act on.
///
/// A watch has no sign-in screen to send anyone to, so "signed out" is not an error
/// here — it is an instruction to go and open the phone. Saying "unauthorised" would
/// be true and useless.
struct Problem: Equatable {
    let message: String
    let systemImage: String

    @MainActor
    init(_ error: Error, identity: WatchIdentity) {
        switch error {
        case APIError.unauthorized:
            // The cached token is the likelier culprit than the sign-in, so it goes
            // and the next attempt asks the phone again.
            identity.refused()
            self = Problem.needsPhone
        default:
            if identity.state == .unavailable {
                self = Problem.needsPhone
            } else {
                self = Problem(
                    message: (error as? LocalizedError)?.errorDescription
                        ?? "Could not reach the list.",
                    systemImage: "exclamationmark.triangle"
                )
            }
        }
    }

    private init(message: String, systemImage: String) {
        self.message = message
        self.systemImage = systemImage
    }

    static let needsPhone = Problem(
        message: "Open Shopping on your phone, then try again.",
        systemImage: "iphone"
    )
}

struct WatchProblemView: View {
    let problem: Problem
    let retry: () -> Void

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: problem.systemImage)
                .font(.title3)
                .foregroundStyle(.secondary)
            Text(problem.message)
                .font(.footnote)
                .multilineTextAlignment(.center)
            Button("Try again", action: retry)
                .font(.footnote)
        }
        .padding(.horizontal, 4)
    }
}
