import SwiftUI

/// Whether this watch is in step with the server, as one dot.
///
/// A wrist has no room for a sentence. The phones can afford "Offline. 2 changes
/// waiting to be sent." because they have a line to spare; here that line costs a row,
/// and a row on this screen is an item you cannot see.
///
/// Two colours, and the reading is deliberately coarse:
///
/// * **Green** — what is on screen came from the server and nothing is waiting to go
///   back. Everything you can see is true and everything you have done has landed.
/// * **Orange** — one of those is not true: either something you did is still queued,
///   or the last look at the server failed and this list is from memory.
///
/// Folding both into one colour is the point. The difference between them is not
/// something anybody acts on mid-shop — either way the answer is "carry on, it will
/// sort itself out" — and a third colour would be a legend to learn for no decision.
struct WatchStatusDot: View {
    /// Changes made here that have not reached the server.
    var waiting: Int
    /// Whether the last look at the server failed.
    var offline: Bool

    private var inStep: Bool { waiting == 0 && !offline }

    var body: some View {
        Circle()
            .fill(inStep ? Color.green : Color.orange)
            .frame(width: 8, height: 8)
            .accessibilityLabel(said)
    }

    /// Spoken in full, because the thing that makes a dot right on a wrist is exactly
    /// what makes it useless to somebody reading by ear.
    private var said: String {
        switch (offline, waiting) {
        case (false, 0): return "Up to date"
        case (true, 0): return "Offline. Showing what was last loaded."
        case (_, let n): return "^[\(n) change](inflect: true) waiting to be sent"
        }
    }
}
