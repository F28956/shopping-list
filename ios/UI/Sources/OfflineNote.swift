import SwiftUI

/// A quiet line above a list that is being shown from the cache.
///
/// Quiet on purpose. Something is on the screen, it may be a little old, and
/// interrupting somebody halfway round a shop about it would be worse than the
/// staleness. The loud case — nothing cached and no connection — is a
/// `ContentUnavailableView` on the screen itself, because there the alternative is
/// the sentence this whole change exists to delete.
struct OfflineNote: View {
    var offline: Bool = true
    /// Changes made here that have not been sent. Shown as a count rather than as
    /// "syncing…", because a person can act on a number — it is the difference between
    /// staying put for a moment and walking out of the shop.
    var waiting: Int = 0

    var body: some View {
        Label(said, systemImage: offline ? "icloud.slash" : "clock.arrow.circlepath")
            .font(.footnote)
            .foregroundStyle(.secondary)
            .accessibilityIdentifier("offline.note")
    }

    /// The three states of `docs/offline.md`, minus the one that interrupts: up to
    /// date, and offline with N changes waiting.
    private var said: String {
        let changes = waiting == 1 ? "change" : "changes"
        switch (offline, waiting) {
        case (true, 0): return "Offline. Showing what was last loaded."
        case (true, _): return "Offline. \(waiting) \(changes) waiting to be sent."
        default: return "\(waiting) \(changes) waiting to be sent."
        }
    }
}
