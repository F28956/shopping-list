#if DEBUG

    import Foundation

    /// Runs the app against a fixed, in-memory world instead of a server.
    ///
    /// UI tests cannot sign in: the flow leaves the app, goes to Google, and asks a
    /// person for a passkey. Nor should they need a server running, a database in a
    /// known state, or a network. So when the app is launched with `-uiTesting`, two
    /// things change and nothing else does — the identity reports a signed-in person
    /// without asking Google, and `URLSession` answers from `StubAPI` rather than the
    /// wire. Every view, view-model and decoding path above that is the real one.
    ///
    /// The whole file is behind `#if DEBUG`, so a release build has no fixture, no
    /// stub protocol, and no launch argument that would reach them. There is a test
    /// asserting exactly that.
    enum UITesting {
        static var isRunning: Bool {
            ProcessInfo.processInfo.arguments.contains("-uiTesting")
        }

        /// Which world to build. Passed as `-uiScenario <name>`.
        static var scenario: String {
            ProcessInfo.processInfo.environment["UI_SCENARIO"] ?? "default"
        }
    }

#endif
