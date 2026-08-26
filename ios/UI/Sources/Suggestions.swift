import Foundation
import Observation

/// What to offer under the add field, and when to go and ask for it.
///
/// One implementation for the phone and the Mac. It was two, and then it was one and
/// a half: the Mac had the state and the rows to show it and nothing that ever filled
/// them, so its add field could never suggest anything. A list that is rendered in
/// two places and populated in one is exactly the shape that hides that.
///
/// The matching and the ordering are the server's. This decides only when to ask.
@MainActor
@Observable
final class Suggestions {
    private(set) var offered: [String] = []

    /// Cancelled on every keystroke, so a slow answer for `mil` cannot arrive after a
    /// fast one for `milk` and put the wrong list back.
    @ObservationIgnored private var asking: Task<Void, Never>?

    /// Long enough that a fast typist makes one request rather than eight.
    private static let settle = Duration.milliseconds(150)

    /// Asks again for what has just been typed.
    ///
    /// Nothing typed, nothing offered — and nothing asked for either. The whole
    /// history is not a suggestion; it is a second list on top of the real one.
    func update(typed: String, fetch: @escaping (String) async throws -> [String]) {
        asking?.cancel()

        let wanted = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !wanted.isEmpty else {
            offered = []
            return
        }

        asking = Task { [weak self] in
            try? await Task.sleep(for: Self.settle)
            guard !Task.isCancelled else { return }

            let found = (try? await fetch(wanted)) ?? []
            guard !Task.isCancelled else { return }

            // Shown as given: how many, and whether what was typed in full comes
            // back, are both decided by the service.
            self?.offered = found
        }
    }

    /// Forgets what was offered, for when the field is emptied or sent.
    func clear() {
        asking?.cancel()
        offered = []
    }
}
