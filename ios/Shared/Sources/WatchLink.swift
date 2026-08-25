import Foundation

/// The words the phone and the watch use to talk to each other.
///
/// Shared rather than written out on both sides. The two halves live in different
/// targets and nothing links them together, so a mistyped key would not fail to
/// build — it would fail to answer, on a watch, in a shop.
enum WatchLink {
    /// Watch asks for a token with this key; the phone replies under the same one.
    static let tokenRequest = "token"
}
