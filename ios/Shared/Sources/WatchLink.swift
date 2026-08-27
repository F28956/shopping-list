import Foundation

/// The words the phone and the watch use to talk to each other.
///
/// Shared rather than written out on both sides. The two halves live in different
/// targets and nothing links them together, so a mistyped key would not fail to
/// build — it would fail to answer, on a watch, in a shop.
enum WatchLink {
    /// Watch asks for a token with this key; the phone replies under the same one.
    static let tokenRequest = "token"
    /// The phone's answer also carries which server that token is for.
    ///
    /// In the same message rather than a second one, because the two are useless
    /// apart: a watch holding a token for a server it cannot name would send it
    /// somewhere, and a watch that knew the address without a token could not use it.
    /// Entering a URL on a watch is not a thing to ask of anybody, so this is the only
    /// way the address can arrive (C5).
    static let serverAddress = "server"
}
