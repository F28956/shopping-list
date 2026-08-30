import Foundation

/// The token inside a share link.
///
/// Whoever receives a link pastes the whole thing, or just the token, or the link
/// with a stray space around it. All three mean the same request, and asking somebody
/// to trim it themselves is asking them to do the computer's job.
func token(in pasted: String) -> String? {
    let trimmed = pasted.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return nil }

    // A link: the token is the fragment of `/join#<token>`, after the `#`.
    //
    // The fragment rather than the path, because a fragment is the one part of a URL
    // a browser never sends -- so a token there is written into no access log on the
    // way to somebody's home server, for the week it stays valid.
    //
    // The last path component is still read, for a link made by an older server that
    // still puts it there. That falls away once nothing issues those any more.
    if let url = URL(string: trimmed), url.scheme != nil {
        if let fragment = url.fragment(percentEncoded: false), !fragment.isEmpty {
            return fragment
        }
        let last = url.lastPathComponent
        return last.isEmpty || last == "/" || last == "join" ? nil : last
    }

    // A bare token: no spaces, and nothing that could be a path.
    guard !trimmed.contains(" "), !trimmed.contains("/") else { return nil }
    return trimmed
}

/// The server a share link came from, if it named one.
///
/// C7. A share link is the ordinary way a second person arrives — often on a phone
/// with no app on it yet — and it carries its own origin. Offering that address turns
/// the worst first run in the product, "somebody sent me a list and the app is asking
/// me for a URL", into one tap.
///
/// **Offered and never adopted.** A link is a bearer credential from an untrusted
/// sender, and pointing an app at a host because a message said so is not something to
/// do without showing the host. The screen shows it and somebody agrees.
///
/// `nil` for a bare token, which names nothing.
func server(in pasted: String) -> ServerAddress? {
    let trimmed = pasted.trimmingCharacters(in: .whitespacesAndNewlines)

    guard let url = URL(string: trimmed),
          let scheme = url.scheme,
          let host = url.host()
    else { return nil }

    // Rebuilt from the parts rather than trimmed from the string, so that whatever
    // `ServerAddress` refuses is refused here too — one set of rules about what an
    // address is, and it lives there.
    var address = "\(scheme)://\(host)"
    if let port = url.port {
        address += ":\(port)"
    }
    address += prefix(in: url)

    return try? ServerAddress.parse(address).get()
}

/// Where the server sits under its host, read out of the link itself.
///
/// A server mounted at `https://example.com/sl` issues
/// `https://example.com/sl/join#token`, and a reader that kept only the host would
/// offer `https://example.com` — an address that either serves somebody else's
/// application or nothing at all. The prefix is everything before the `/join`
/// segment, so it is read from the link rather than asked of the person pasting it.
///
/// `""` for a link with no `join` in it, which is the shape this was written for
/// before there were prefixes and is still what an unrecognised link should offer.
private func prefix(in url: URL) -> String {
    var segments = url.path.split(separator: "/").map(String.init)

    // The older shape puts the token in the path, so the last segment is the token
    // rather than `join`. Dropped first, leaving both shapes looking the same.
    if segments.count >= 2, segments[segments.count - 2] == "join" {
        segments.removeLast()
    }

    guard segments.last == "join" else { return "" }
    segments.removeLast()

    return segments.isEmpty ? "" : "/" + segments.joined(separator: "/")
}
