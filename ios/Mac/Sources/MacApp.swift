import AuthenticationServices
import SwiftUI

@main
struct ShoppingListMacApp: App {
    @State private var identity = Identity()

    var body: some Scene {
        WindowGroup {
            MacRootView()
                .environment(identity)
                .frame(minWidth: 620, minHeight: 420)
                .task { await identity.restore() }
        }
        // A list is a document-shaped thing: one window, resizable, remembered.
        .defaultSize(width: 820, height: 560)
        .commands {
            // Nothing under it yet, but replacing New Window's default here stops
            // the menu offering a second window that would fight the first for the
            // same list selection.
            CommandGroup(replacing: .newItem) {}
        }
    }
}

struct MacRootView: View {
    @Environment(Identity.self) private var identity

    var body: some View {
        switch identity.state {
        case .unknown:
            ProgressView()
        case .signedOut:
            MacSignInView()
        case .signedIn:
            MacShoppingView(
                api: API(
                    baseURL: Config.apiBaseURL,
                    token: { await identity.token() },
                    remembered: { identity.isRemembered }
                )
            )
        }
    }
}

struct MacSignInView: View {
    @Environment(Identity.self) private var identity

    var body: some View {
        VStack(spacing: 16) {
            Text("Shopping list")
                .font(.largeTitle.weight(.semibold))
            Text("The same lists as the phone, with a keyboard.")
                .foregroundStyle(.secondary)

            // Apple's own button rather than one styled to look like it: the mark,
            // the wording and the corner radius are the part people recognise before
            // they read anything, and it is what the guidelines ask for besides.
            SignInWithAppleButton(.signIn, onRequest: identity.request) { result in
                Task { await identity.adopt(result) }
            }
            .signInWithAppleButtonStyle(.automatic)
            .frame(width: 240, height: 40)
            .accessibilityIdentifier("sign-in")

            if let error = identity.lastError {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        }
        .padding(40)
    }

}
