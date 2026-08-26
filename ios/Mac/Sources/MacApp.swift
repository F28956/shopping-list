import GoogleSignIn
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
                .onOpenURL { GIDSignIn.sharedInstance.handle($0) }
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
                api: API(baseURL: Config.apiBaseURL, token: { await identity.token() })
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

            if identity.isConfigured {
                Button("Sign in with Google") { Task { await signIn() } }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
            } else {
                Text("This build has no Google client id yet.\nSee ios/README.md.")
                    .font(.footnote)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
            }

            if let error = identity.lastError {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        }
        .padding(40)
    }

    @MainActor
    private func signIn() async {
        // The sheet hangs off a window here rather than a view controller. Whichever
        // is in front will do: there is only ever one.
        guard let window = NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first
        else { return }
        await identity.signIn(presenting: window)
    }
}
