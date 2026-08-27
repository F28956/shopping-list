import Foundation

/// Where the server is.
///
/// A thin front for `ServerDirectory`, kept because every call site says
/// `Config.apiBaseURL` and the question "which server" now has a longer answer than a
/// build setting. What changed is only where the answer comes from: what somebody
/// entered, then what the build was pointed at, then `localhost`.
///
/// `localhost` is the device itself, which is the first thing to get wrong once this
/// leaves the simulator — and on a fresh install with nothing configured it is a
/// placeholder rather than a destination, because the address has not been asked for
/// yet. `ServerDirectory.needsAnAddress` is the question to ask before using this.
enum Config {
    static var apiBaseURL: URL {
        ServerDirectory.current?.url ?? URL(string: "http://localhost:8080")!
    }
}
