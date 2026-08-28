import SwiftUI

/// What this app can offer, as opposed to how it fetches things.
///
/// The data path stopped asking about the mode when `Backend` was drawn: a screen is
/// handed one and cannot tell whether it talks to a server or to the device's own. What
/// remained were sixty-odd `onDeviceOnly` checks, and reading them showed they were
/// never really about the mode either. They were about three questions:
///
/// * **Is there anybody to share with?** A share link names a server.
/// * **Is there an account?** Signing in and out, and who may sign in.
/// * **Is there a far end that can be out of reach?** The status dot, the offline note,
///   and the difference between "you have no lists" and "I could not find out".
///
/// Three booleans that happen to agree today, and are named apart on purpose. Each site
/// now says *why* something is hidden rather than which mode it is in -- and the day a
/// list is shared device-to-device with no server anywhere, ``sharing`` turns on while
/// ``accounts`` stays off. That combination cannot be expressed by the flag it replaces.
///
/// Hiding rather than refusing is the right shape and always was: offering to share when
/// there is nobody to share with is a worse app, not a more uniform one. Nothing here is
/// a security boundary -- every route behind these is refused in the service layer to
/// anybody who may not use it.
struct Capabilities: Equatable, Sendable {

    /// Share links, joining, and who else is on a list.
    var sharing: Bool

    /// Signing in and out, and who may sign in to the server.
    var accounts: Bool

    /// Whether there is a far end that can be out of reach.
    ///
    /// False on a device answering for itself, where "offline" is not a state that
    /// exists: nothing is stale, nothing is waiting for a connection that is coming, and
    /// a dot reporting the health of a connection that does not exist is an indicator
    /// somebody has to learn to ignore.
    var syncing: Bool

    /// A device with a server: everything.
    static let withAServer = Capabilities(sharing: true, accounts: true, syncing: true)

    /// A device kept to itself. The default, and not a degraded one -- a shopping list
    /// is meant to be useful the moment it is installed.
    static let onItsOwn = Capabilities(sharing: false, accounts: false, syncing: false)

    /// What this device can do, as things stand.
    static var current: Capabilities {
        ServerDirectory.isOnDeviceOnly ? .onItsOwn : .withAServer
    }
}

extension EnvironmentValues {
    /// Read by every screen that shows or hides something, so none of them reads
    /// `ServerDirectory` for itself.
    ///
    /// The environment rather than an argument threaded through: this is ambient, every
    /// screen wants it, and passing it by hand is how `StatusDot` came to take a
    /// parameter that three of its four callers had to remember to set.
    @Entry var capabilities: Capabilities = .current
}
