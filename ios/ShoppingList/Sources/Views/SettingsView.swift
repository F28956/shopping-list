import SwiftUI

/// Where a server is configured, and where it stops being one.
///
/// The app opens without any of this, and that is the point: a shopping list is
/// usable the moment it is installed. Somebody who runs a server comes here to say
/// so, which is a thing a minority of people do once — exactly what a settings screen
/// is for.
struct SettingsView: View {
    let cache: Cache
    let api: API
    /// Whether this person administers the server. Decides whether the screen that
    /// manages who may sign in exists at all.
    let isOwner: Bool
    /// Asks the lists screen to open the join sheet, once this one is out of the way.
    ///
    /// Joining is an action rather than a setting, and it lives here only because it
    /// needs a server — which makes it one of the things that is absent more often
    /// than it is present, and this is the screen for those.
    let joinAList: () -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(\.capabilities) private var capabilities
    @Environment(Identity.self) private var identity

    @State private var choosing = false
    @State private var leaving = false
    @State private var managingServer = false
    @State private var managingTags = false

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                if ServerDirectory.current != nil {
                    Section {
                        Button("Join a list", systemImage: "person.badge.plus") {
                            dismiss()
                            joinAList()
                        }
                        .accessibilityIdentifier("join-a-list")

                        if isOwner {
                            // A sheet rather than a push: `ServerPeopleView` brings its
                            // own navigation stack and its own Done, because it is
                            // presented from the Mac too.
                            Button("Who may sign in", systemImage: "person.2.badge.key") {
                                managingServer = true
                            }
                            .accessibilityIdentifier("manage-server")
                        }
                    } header: {
                        Text("Lists")
                    }
                }

                // Global, so not on a list. Whoever may change them differs: with a
                // server they are the household's vocabulary and the owner's to
                // change; with none there is no household, so anybody using the app
                // may. Absent rather than refusing for somebody who may not -- the
                // service hides the routes from them anyway.
                if !capabilities.accounts || isOwner {
                    Section {
                        Button("Categories", systemImage: "tag") { managingTags = true }
                            .accessibilityIdentifier("manage-tags")
                    } footer: {
                        Text("The categories items are grouped under.")
                    }
                }

                Section {
                    if let server = ServerDirectory.current {
                        LabeledContent("Server", value: server.origin)
                        Button("Stop using this server", role: .destructive) { leaving = true }
                            .accessibilityIdentifier("leave-server")
                    } else {
                        LabeledContent("Server", value: "None")
                        Button("Use a server") { choosing = true }
                            .accessibilityIdentifier("choose-server")
                    }
                } header: {
                    Text("Syncing")
                } footer: {
                    Text(
                        ServerDirectory.current == nil
                            ? """
                              Your lists are on this phone and nowhere else. Add a \
                              server to sync them between devices and share them with \
                              other people.
                              """
                            : """
                              Your lists sync with this server and can be shared with \
                              other people on it.
                              """
                    )
                }

                // Last, and below the server: it is the section a person opens once,
                // when something has gone wrong and somebody has asked them to.
                DiagnosticsSettings()
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .sheet(isPresented: $managingTags) {
                TagsView(cache: cache, api: api, onDeviceOnly: ServerDirectory.isOnDeviceOnly)
            }
            .sheet(isPresented: $managingServer) {
                ServerPeopleView(api: api)
            }
            .sheet(isPresented: $choosing) {
                ServerAddressView(
                    accepted: { address, _ in
                        // C4 in the other direction: what is here was made with no
                        // server, and it is about to be sent to one. Nothing is thrown
                        // away — adding a server is not the destructive half of
                        // changing one.
                        ServerDirectory.remember(address)
                        choosing = false
                        dismiss()
                    },
                    // Back to settings, with the answer unchanged. Somebody who opened
                    // this to read it is not somebody who has agreed to anything.
                    cancelled: { choosing = false }
                )
            }
            // C4. The cache holds rows keyed by ids and uuids that server minted, and
            // history and suggestions belong to an account on it. Keeping them would
            // show one server's lists under no server's name.
            .alert("Stop using this server?", isPresented: $leaving) {
                Button("Cancel", role: .cancel) {}
                Button("Stop", role: .destructive) {
                    cache.forgetEverything()
                    identity.signOut()
                    ServerDirectory.forget()
                    dismiss()
                }
            } message: {
                Text(
                    """
                    This signs you out and removes everything stored on this device. \
                    Anything still waiting to be sent will be lost.
                    """
                )
            }
        }
    }

}
