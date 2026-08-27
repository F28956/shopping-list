import SwiftUI

/// Whether this watch is in step with its phone, as one dot.
///
/// A wrist has no room for a sentence. The phones can afford "Offline. 2 changes
/// waiting to be sent." because they have a line to spare; here that line costs a row,
/// and a row on this screen is an item you cannot see.
///
/// Two colours, and the reading is deliberately coarse:
///
/// * **Green** — everything you have done here has reached the phone.
/// * **Orange** — something you did is still waiting for the phone to come into range.
///
/// There is no "offline" any more, and its absence is the point. This watch does not
/// talk to a server, so it cannot be out of touch with one; the only question it can
/// answer is whether the phone has heard, and that is what the dot says.
struct WatchStatusDot: View {
    /// Ticks made here that have not reached the phone.
    var waiting: Int
    /// Whether there is a server anywhere in this arrangement, which changes what
    /// "reached the phone" means — with no server the phone is the end of the journey,
    /// so nothing is in transit to anywhere else.
    var onDeviceOnly: Bool

    var body: some View {
        Circle()
            .fill(waiting == 0 ? Color.green : Color.orange)
            .frame(width: 8, height: 8)
            .accessibilityLabel(said)
    }

    /// Spoken in full, because the thing that makes a dot right on a wrist is exactly
    /// what makes it useless to somebody reading by ear.
    private var said: String {
        if waiting == 0 {
            return onDeviceOnly ? "On your phone" : "Up to date"
        }
        return "^[\(waiting) change](inflect: true) waiting for your phone"
    }
}
