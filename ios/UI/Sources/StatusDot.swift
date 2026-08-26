import SwiftUI

/// Whether this device is in step with the server, as one dot.
///
/// The same two colours the watch uses, for the same reason: it is the one signal you
/// can read without stopping. The phones keep their sentence as well — they have a
/// line to spare and it says how many and why — but the sentence only appears when
/// something is wrong, and a dot that is green the rest of the time is the difference
/// between "nothing is wrong" and "nothing has been checked".
///
/// * **Green** — what is on screen came from the server and nothing is waiting to go
///   back.
/// * **Orange** — one of those is not true: either something you did is still queued,
///   or the last look at the server failed and this is from memory.
struct StatusDot: View {
    /// Changes made here that have not reached the server.
    var waiting: Int
    /// Whether the last look at the server failed.
    var offline: Bool

    private var inStep: Bool { waiting == 0 && !offline }

    var body: some View {
        Circle()
            .fill(inStep ? Color.green : Color.orange)
            .frame(width: 9, height: 9)
            .accessibilityLabel(said)
    }

    /// Spoken in full, because what makes a dot right at a glance is exactly what makes
    /// it useless to somebody reading by ear.
    private var said: String {
        switch (offline, waiting) {
        case (false, 0): return "Up to date"
        case (true, 0): return "Offline. Showing what was last loaded."
        case (_, let n): return "^[\(n) change](inflect: true) waiting to be sent"
        }
    }
}
