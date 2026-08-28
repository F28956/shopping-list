// Not on the watch, and not by accident.
//
// `Store/Sources` is compiled into all three apps, but a watch is not a server and
// never will be: it has no database of its own worth the name and gets everything from
// the phone it is paired to -- see `WatchStore`. The guard is on the platform rather
// than on `canImport`, because the header *is* visible to the watch target (they share
// `Parser/include`) and only the library is absent. A `canImport` check would compile
// and then fail at the link, which is a worse way to learn this.
#if !os(watchOS)

import EmbeddedC
import Foundation

/// The server, on this device.
///
/// A thin Swift skin over `web/embedded`, which links `domain` -- the server's own
/// crate, over the server's own schema. See that crate's documentation for why: in
/// short, standalone is currently implemented as *a server that fails every request*,
/// and every screen has had to be taught that the failure is not real.
///
/// **This is not yet wired to anything.** It exists so that the cost of linking the
/// thing can be measured before a client is built on top of it, which is a much
/// cheaper moment to change one's mind. `open` is called once at launch and the answer
/// is thrown away.
///
/// The next step is a `Backend` conformer over this -- see `Backend.swift`, where the
/// surface it would have to satisfy is already written down.
enum LocalServer {

    /// Where the device's database lives.
    ///
    /// Beside the cache rather than inside it: they are two different things for now
    /// and one of them is meant to replace the other, so keeping them apart is what
    /// makes the switch reversible.
    static var location: URL {
        let folder = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("ShoppingList", isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder.appendingPathComponent("device.sqlite")
    }

    /// Opens it, and says whether it opened.
    ///
    /// Deliberately does the smallest real thing: a database that opens has been
    /// migrated by `domain`'s own migrator and has this device's person in it, so a
    /// `true` here is the whole embedding working rather than a symbol resolving.
    @discardableResult
    static func check() -> Bool {
        guard let handle = location.path.withCString({ embedded_open($0) }) else {
            return false
        }
        defer { embedded_close(handle) }

        // Non-zero means `identity::from_claims` ran and the device has a person, which
        // means the migrations ran, which means the schema is the server's.
        return embedded_me(handle) != 0
    }
}

#endif
