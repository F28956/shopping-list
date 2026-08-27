import SwiftUI

/// Whether this watch is in step with the far end, as one dot.
///
/// A wrist has no room for a sentence. The phones can afford "Offline. 2 changes
/// waiting to be sent." because they have a line to spare; here that line costs a row,
/// and a row on this screen is an item you cannot see.
///
/// Two colours, and the reading is deliberately coarse:
///
/// * **Green** — what is on screen is current and nothing is waiting to go back.
/// * **Orange** — one of those is not true: either something you did is still queued,
///   or the last look failed and this list is from memory.
///
/// Folding both into one colour is the point. The difference between them is not
/// something anybody acts on mid-shop — either way the answer is "carry on, it will
/// sort itself out" — and a third colour would be a legend to learn for no decision.
///
/// It says nothing about *which* far end. With a server that is the server; with none
/// it is the phone. The watch behaves the same either way and so does this — see
/// `WatchLink`.
struct WatchStatusDot: View {
    /// Changes made here that have not reached the far end.
    var waiting: Int
    /// Whether the last attempt to reach it failed.
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
        case (true, 0): return "Out of touch. Showing what was last loaded."
        case (_, let n): return "^[\(n) change](inflect: true) waiting to be sent"
        }
    }
}
