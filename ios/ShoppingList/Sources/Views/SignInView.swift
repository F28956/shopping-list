import AuthenticationServices
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

            // Apple's own button rather than one styled to look like it: the mark,
            // the wording and the corner radius are the part people recognise before
            // they read anything, and it is what the guidelines ask for besides.
            SignInWithAppleButton(.signIn, onRequest: identity.request) { result in
                Task { await identity.adopt(result) }
            }
            .signInWithAppleButtonStyle(.black)
            .frame(maxWidth: 280, maxHeight: 48)
            .accessibilityIdentifier("sign-in")

            if let error = identity.lastError {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(32)
    }

}
