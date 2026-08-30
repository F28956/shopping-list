import Foundation

/// Where the server is, as a thing that has been checked rather than a string.
///
/// A self-hosted app has to ask, because the answer is different for everybody. What
/// it must not do is take what was typed and hope: a person pastes what is in their
/// browser's location bar, which has a path on it, and the failure that causes is
/// silent and much later.
///
/// **The trap this type exists to close.** Requests used to be built with
/// `URL(string: path, relativeTo: base)`, which resolves an *absolute* path against
/// the base's host and throws the base's own path away — so a server mounted at
/// `https://example.com/sl` would have been asked for `https://example.com/api/lists`
/// and told nothing about it. Deciding the shape once, here, and handing out whole
/// URLs through ``url(for:)`` is the only way to close that for every call site at
/// once. Nothing outside this type appends to an address.
struct ServerAddress: Equatable {
    /// `scheme://host` or `scheme://host:port`, lowercased, with no trailing slash.
    let origin: String

    /// Where the server is mounted under that origin: `""` at the root, or a path
    /// beginning with `/` and not ending in one.
    ///
    /// One domain often has several things behind it, and a server at
    /// `https://example.com/sl` is an ordinary arrangement rather than a mistake —
    /// insisting on a whole host would be a constraint on somebody's DNS. The server
    /// end of this is `BASE_PATH`.
    var prefix: String = ""

    /// The address as somebody would type it, and as it is stored.
    var written: String { origin + prefix }

    var url: URL { URL(string: written)! }

    /// The URL for one of this application's own paths, such as `/api/lists`.
    ///
    /// Concatenated rather than resolved. URL resolution has rules about absolute
    /// paths that are correct and are not what is wanted here: `/api/lists` against
    /// `https://example.com/sl` resolves to `https://example.com/api/lists`, losing
    /// the prefix silently. Joining the strings has no such rule to trip over.
    func url(for path: String) -> URL? {
        assert(path.hasPrefix("/"), "url(for:) takes an absolute path, got \(path)")
        return URL(string: written + path)
    }

    /// Why an address could not be used, in the words the screen says.
    enum Problem: LocalizedError, Equatable {
        case empty
        case notAnAddress
        /// C6. Release builds accept `https://` only: an app that can be pointed
        /// anywhere and permits cleartext puts somebody's shopping and their bearer
        /// token on whatever café Wi-Fi they are on.
        case insecure
        /// A query or a fragment — refused rather than silently dropped, because
        /// dropping part of what somebody typed is how they end up at the wrong
        /// server believing they are at the right one.
        ///
        /// A *path* is no longer refused: it is kept, as the prefix the server is
        /// mounted under. See ``prefix``.
        case notJustAnOrigin

        /// `LocalizedError`, so that a screen reaching for `localizedDescription`
        /// gets these words rather than "the operation could not be completed,
        /// ServerAddress.Problem error 3". A bare `Error` renders as its case index,
        /// and the Mac settings window showed exactly that to anybody who typed an
        /// address with a path on the end.
        var errorDescription: String? { sentence }

        var sentence: String {
            switch self {
            case .empty:
                "Enter the address of your Shopping List server."
            case .notAnAddress:
                "That does not look like an address."
            case .insecure:
                "Addresses must start with https://"
            case .notJustAnOrigin:
                "Enter the address without a ? query or # fragment."
            }
        }
    }

    /// Reads what somebody typed, repairing the obvious and refusing the ambiguous.
    ///
    /// Repaired: a missing scheme becomes `https://`, a trailing slash goes, the host
    /// is lowercased, and surrounding whitespace is ignored — all of which are what
    /// the person meant beyond doubt.
    ///
    /// Kept: a path, which is the prefix the server is mounted under — see
    /// ``prefix``. A lone trailing slash is not a path and goes.
    ///
    /// Refused: a query or a fragment. Those are *not* beyond doubt; see the note on
    /// the type.
    ///
    /// There is deliberately **no way to ask for cleartext**. There used to be a
    /// parameter for it, and five call sites passed `true` -- including the one that
    /// reads a host out of a pasted share link, which is untrusted text from whoever
    /// sent it. The release guarantee then held only because the one path that stores
    /// an address happened to use the default. An invariant that depends on five
    /// callers agreeing is not an invariant.
    ///
    /// Nothing was lost by removing it. A debug build allows cleartext through
    /// `allowsCleartext` anyway, which is the case the parameter existed for; a release
    /// build refuses `http://` everywhere, including from a pasted link, which is what
    /// it should always have done.
    static func parse(_ typed: String) -> Result<ServerAddress, Problem> {
        let trimmed = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return .failure(.empty) }

        // A missing scheme is the commonest thing typed, and `URLComponents` reads
        // "example.com:8080" as scheme `example.com` — so this is decided before
        // parsing rather than repaired after it.
        let withScheme = trimmed.contains("://") ? trimmed : "https://\(trimmed)"

        guard let parts = URLComponents(string: withScheme),
              let scheme = parts.scheme?.lowercased(),
              let host = parts.host?.lowercased(),
              !host.isEmpty
        else { return .failure(.notAnAddress) }

        guard scheme == "https" || scheme == "http" else { return .failure(.notAnAddress) }
        guard scheme == "https" || allowsCleartext else { return .failure(.insecure) }

        guard parts.query == nil, parts.fragment == nil else { return .failure(.notJustAnOrigin) }

        // A lone trailing slash is what a browser's location bar shows and is not a
        // path anybody meant. Anything more is the prefix the server is mounted under,
        // kept with no trailing slash so that `written + "/api/lists"` has one slash
        // between the two and never two.
        let prefix = parts.path == "/" ? "" : String(parts.path.reversed().drop { $0 == "/" }.reversed())

        // Reassembled rather than trimmed, so the stored form is the one this type
        // promises whatever arrived.
        var origin = "\(scheme)://\(host)"
        if let port = parts.port, port != defaultPort(for: scheme) {
            origin += ":\(port)"
        }

        return .success(ServerAddress(origin: origin, prefix: prefix))
    }

    private static func defaultPort(for scheme: String) -> Int {
        scheme == "https" ? 443 : 80
    }

    /// C6: cleartext in debug builds, where the server is on the same desk, and never
    /// in a release one.
    static var allowsCleartext: Bool {
        #if DEBUG
            true
        #else
            false
        #endif
    }
}
