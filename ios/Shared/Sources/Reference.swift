import Foundation

/// The units and tags every list can rely on being there.
///
/// Normally these come from the server, which is the authority on them. But they are
/// the one part of this application's data that is the *same everywhere* — seeded by
/// migration, writable only by the process itself, belonging to no user — so a device
/// with no server can have them too, and must: without units an item has no measure,
/// and without tags a list has no aisles.
///
/// The file is `reference/reference.json`, shared with the server and guarded by
/// `domain::reference::the_seed_and_the_file_agree`, which fails if it and the
/// migrations ever disagree.
///
/// **The ids are the point, not just the names.** An item added here carries its
/// `unit_id` when a server finally hears about it, so the numbers have to be the
/// server's numbers.
enum Reference {
    /// Read once. It is a few kilobytes and it never changes within a run.
    static let units: [Unit] = loaded?.units ?? []
    static let tags: [Tag] = loaded?.tags ?? []

    private struct File: Decodable {
        let units: [Unit]
        let tags: [Tag]
    }

    private static let loaded: File? = {
        guard let url = Bundle.main.url(forResource: "reference", withExtension: "json"),
              let data = try? Data(contentsOf: url)
        else {
            // A build that forgot the resource. Empty rather than a crash: the app
            // still works with a server, which is the case that would notice.
            assertionFailure("reference.json is missing from the bundle")
            return nil
        }

        return try? JSONDecoder().decode(File.self, from: data)
    }()
}
