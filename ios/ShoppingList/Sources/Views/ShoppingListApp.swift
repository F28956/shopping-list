import SwiftUI

@main
struct ShoppingListApp: App {
    @State private var identity = Identity()

    init() {
        let identity = Identity()
        _identity = State(initialValue: identity)

        // The phone is where the watch gets its config, and -- when there is no server
        // -- where its queue goes. See `WatchLink`. Started here rather than held as
        // state: it is a singleton because `WCSession` has one delegate, and a second
        // one would silently take over from the first.
        PhoneLink.shared.token = { await identity.token() }
        PhoneLink.shared.start()

    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(identity)
                // Only when there is somewhere to sign in to. On a device kept to
                // itself there is nobody to restore and nothing that would use the
                // answer.
                .task {
                    if !ServerDirectory.isOnDeviceOnly { await identity.restore() }
                }
                // The sign-in flow leaves the app and comes back through this URL.
        }
    }
}

struct RootView: View {
    @Environment(Identity.self) private var identity

    /// Re-read when settings change the answer, because `ServerDirectory` is storage
    /// rather than observable state and nothing would otherwise tell SwiftUI.
    @State private var hasServer = !ServerDirectory.isOnDeviceOnly

    /// What this app can offer, supplied once for every screen below.
    ///
    /// Held as state rather than read where it is used, so that choosing a server in
    /// Settings changes what is offered without relaunching -- the same reason
    /// `hasServer` is state. Every screen reads it from the environment; none of them
    /// reads `ServerDirectory`.
    @State private var capabilities = Capabilities.current

    /// What every screen below reads, and what the watch is told about. See ``open()``.
    @State private var backend: (any Backend)?
    @State private var api: API?

    var body: some View {
        Group {
            if hasServer {
                signedInOrNot
            } else {
                // The default, and it opens straight into the lists. A shopping list
                // should be usable the moment it is installed, not open by asking a
                // question about hosting — so there is no first-run screen, no sheet
                // to dismiss, and nothing to sign in to. Somebody who has a server
                // goes and says so in settings.
                lists
            }
        }
        // Settings is the only thing that changes this, and it changes it under our
        // feet, so the answer is re-read rather than remembered from launch.
        .environment(\.capabilities, capabilities)
        .task { open() }
        .onReceive(NotificationCenter.default.publisher(for: .serverChanged)) { _ in
            hasServer = !ServerDirectory.isOnDeviceOnly
            capabilities = .current
            // A different mode is a different backend -- the device's own server, or a
            // cache in front of somebody else's.
            open()
        }
    }

    @ViewBuilder
    private var lists: some View {
        if let backend, let api {
            ListsView(api: api, backend: backend)
        } else {
            ProgressView()
        }
    }

    /// Built once and kept, rather than made afresh every time a body runs.
    ///
    /// This was a computed property, so every re-render opened another database and
    /// started another set of watchers, and `PhoneLink` pointed at whichever one
    /// happened to be made last. Held as state, and rebuilt only when the answer
    /// actually changes -- which is when somebody chooses a server, or stops using one.
    private func open() {
        let api = API(
            baseURL: Config.apiBaseURL,
            token: { await identity.token() },
            remembered: { identity.isRemembered }
        )
        // The device answers for itself when nobody has chosen a server, unless its
        // database will not open -- which falls back to the old path, cache and all.
        let backend: any Backend = (ServerDirectory.isOnDeviceOnly
            ? LocalBackend.readyForUse()
            : nil) ?? CachingBackend(remote: api)

        self.api = api
        self.backend = backend

        // The watch is told what *this* holds, so it has to be told by the same thing
        // this reads. It used to read `Cache.shared` directly, which stopped being the
        // phone's memory the day the device's own server took over: after a migration
        // the watch would have gone on receiving the picture taken the moment it ran,
        // for ever, with no error anywhere.
        PhoneLink.shared.use(backend)
    }

    @ViewBuilder
    private var signedInOrNot: some View {
        switch identity.state {
        case .unknown:
            ProgressView()
        case .signedOut:
            SignInView()
        case .signedIn:
            lists
        }
    }
}
