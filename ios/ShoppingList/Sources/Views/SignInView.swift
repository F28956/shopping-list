import AuthenticationServices
import SwiftUI

/// Signing in to the server this device has been pointed at.
///
/// Reached only from `RootView`, and only once somebody has configured a server, so it
/// is never what a fresh install opens on.
///
/// It leads back out as well as in, and it has to. This screen used to be the whole
/// app: somebody who went into settings to see what "Use a server" said arrived here,
/// found an Apple button and nothing else, and was held here until they signed in to a
/// stranger's server -- with the settings that put them here now behind sign-in. The
/// only way out was to delete the app. A screen somebody cannot leave is a screen that
/// has taken the phone off them.
struct SignInView: View {
    @Environment(Identity.self) private var identity

    private let cache = Cache.shared

    /// Which server this is, because "change server" and "use this device only" are
    /// not decisions anybody can make without knowing what they are leaving.
    private let server = ServerDirectory.current

    @State private var choosing = false
    @State private var leaving = false

    var body: some View {
        VStack(spacing: 20) {
            Text("Shopping list")
                .font(.largeTitle.weight(.semibold))
            Text("Your lists, on the phone you take to the shop.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            if let server {
                LabeledContent("Server", value: server.origin)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: 280)
            }

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

            // The two ways out, and they are the two mistakes somebody makes here: the
            // address was wrong, or a server was never wanted in the first place.
            VStack(spacing: 12) {
                Button("Use a different server") { choosing = true }
                    .accessibilityIdentifier("change-server")

                Button("Use this device only") { leaving = true }
                    .accessibilityIdentifier("leave-server")
            }
            .font(.footnote)
        }
        .padding(32)
        .sheet(isPresented: $choosing) {
            ServerAddressView { address, _ in
                // C4. A different server mints different ids, so what the last one put
                // in the cache cannot stay -- `remember` says whether it is a different
                // one, and only then is anything thrown away. Retyping the address of
                // the server already configured is a correction, not a move.
                if ServerDirectory.remember(address) {
                    cache.forgetEverything()
                    identity.signOut()
                }
                choosing = false
            }
        }
        // C4 again, and this time the cache goes whatever happens: the lists on screen
        // after this belong to no server, and rows keyed by ids that one minted would
        // be showing its lists under nobody's name. Said out loud rather than done
        // quietly, in the same words settings uses, because it is the same act.
        .alert("Use this device only?", isPresented: $leaving) {
            Button("Cancel", role: .cancel) {}
            Button("Use this device only", role: .destructive) {
                cache.forgetEverything()
                identity.signOut()
                ServerDirectory.onlyThisDevice()
            }
        } message: {
            Text(
                """
                Your lists will stay on this phone and nothing will be synced. This \
                removes everything stored on this device, including anything still \
                waiting to be sent. You can add a server again in settings.
                """
            )
        }
    }

}
