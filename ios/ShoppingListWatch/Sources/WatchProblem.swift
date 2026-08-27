import SwiftUI

/// What went wrong, in terms a watch can act on.
///
/// A watch has no sign-in screen to send anyone to, so "signed out" is not an error
/// here — it is an instruction to go and open the phone. Saying "unauthorised" would
/// be true and useless.
struct Problem: Equatable {
    let message: String
    let systemImage: String

    init(_ error: Error) {
        switch error {
        case APIError.unauthorized:
            // Not "unauthorised", which is true and useless on a wrist. The credential
            // came from the phone and only the phone can produce another, so the thing
            // to say is the thing to do. Throwing the stale one away is the caller's
            // job -- see `WatchStore.credentialRefused`.
            self = Problem.needsPhone
        case is PhoneDestination.OutOfReach:
            self = Problem.needsPhone
        default:
            self = Problem(
                message: (error as? LocalizedError)?.errorDescription
                    ?? "Could not reach the list.",
                systemImage: "exclamationmark.triangle"
            )
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
