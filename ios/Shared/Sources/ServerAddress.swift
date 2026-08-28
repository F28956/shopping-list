import Foundation

/// Where the server is, as a thing that has been checked rather than a string.
///
/// A self-hosted app has to ask, because the answer is different for everybody. What
/// it must not do is take what was typed and hope: a person pastes what is in their
/// browser's location bar, which has a path on it, and the failure that causes is
/// silent and much later.
///
/// **The trap this type exists to close.** Requests are built with
/// `URL(string: path, relativeTo: base)`, which resolves against the base's
/// *directory*. So `https://example.com/lists` as a base silently loses `/lists` the
/// moment a relative path is appended, and `https://example.com/lists/` does not.
/// Deciding the shape once, here, is the only place that can be closed for every call
/// site at once — which is why this stores an origin and refuses anything else.
struct ServerAddress: Equatable {
    /// `scheme://host` or `scheme://host:port`, lowercased, with no trailing slash.
    let origin: String

    var url: URL { URL(string: origin)! }

    /// Why an address could not be used, in the words the screen says.
    enum Problem: Error, Equatable {
        case empty
        case notAnAddress
        /// C6. Release builds accept `https://` only: an app that can be pointed
        /// anywhere and permits cleartext puts somebody's shopping and their bearer
        /// token on whatever café Wi-Fi they are on.
        case insecure
        /// A path, query or fragment — refused rather than silently dropped, because
        /// dropping part of what somebody typed is how they end up at the wrong
        /// server believing they are at the right one.
        case notJustAnOrigin

        var sentence: String {
            switch self {
            case .empty:
                "Enter the address of your Shopping List server."
            case .notAnAddress:
                "That does not look like an address."
            case .insecure:
                "Addresses must start with https://"
            case .notJustAnOrigin:
                "Enter just the address, with no path after it."
            }
        }
    }

    /// Reads what somebody typed, repairing the obvious and refusing the ambiguous.
    ///
    /// Repaired: a missing scheme becomes `https://`, a trailing slash goes, the host
    /// is lowercased, and surrounding whitespace is ignored — all of which are what
    /// the person meant beyond doubt.
    ///
    /// Refused: a path, a query or a fragment. Those are *not* beyond doubt; see the
    /// note on the type.
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

        // A lone trailing slash is what a browser's location bar shows and is not a
        // path anybody meant. Anything more is.
        let path = parts.path
        guard path.isEmpty || path == "/" else { return .failure(.notJustAnOrigin) }
        guard parts.query == nil, parts.fragment == nil else { return .failure(.notJustAnOrigin) }

        // Reassembled rather than trimmed, so the stored form is the one this type
        // promises whatever arrived.
        var origin = "\(scheme)://\(host)"
        if let port = parts.port, port != defaultPort(for: scheme) {
            origin += ":\(port)"
        }

        return .success(ServerAddress(origin: origin))
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
