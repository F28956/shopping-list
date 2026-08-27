import SwiftUI

/// Where a server is configured, and where it stops being one.
///
/// The app opens without any of this, and that is the point: a shopping list is
/// usable the moment it is installed. Somebody who runs a server comes here to say
/// so, which is a thing a minority of people do once — exactly what a settings screen
/// is for.
struct SettingsView: View {
    let cache: Cache

    @Environment(\.dismiss) private var dismiss
    @Environment(Identity.self) private var identity

    @State private var choosing = false
    @State private var leaving = false

    var body: some View {
        NavigationStack {
            SwiftUI.List {
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
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .sheet(isPresented: $choosing) {
                ServerAddressView { address, _ in
                    // C4 in the other direction: what is here was made with no server,
                    // and it is about to be sent to one. Nothing is thrown away —
                    // adding a server is not the destructive half of changing one.
                    ServerDirectory.remember(address)
                    choosing = false
                    dismiss()
                }
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
