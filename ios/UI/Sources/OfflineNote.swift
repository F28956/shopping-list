import SwiftUI

/// A quiet line above a list that is being shown from the cache.
///
/// Quiet on purpose. Something is on the screen, it may be a little old, and
/// interrupting somebody halfway round a shop about it would be worse than the
/// staleness. The loud case — nothing cached and no connection — is a
/// `ContentUnavailableView` on the screen itself, because there the alternative is
/// the sentence this whole change exists to delete.
struct OfflineNote: View {
    var body: some View {
        Label("Offline. Showing what was last loaded.", systemImage: "icloud.slash")
            .font(.footnote)
            .foregroundStyle(.secondary)
            .accessibilityIdentifier("offline.note")
    }
}
