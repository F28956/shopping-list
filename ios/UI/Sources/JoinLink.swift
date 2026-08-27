import Foundation

/// The token inside a share link.
///
/// Whoever receives a link pastes the whole thing, or just the token, or the link
/// with a stray space around it. All three mean the same request, and asking somebody
/// to trim it themselves is asking them to do the computer's job.
func token(in pasted: String) -> String? {
    let trimmed = pasted.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return nil }

    // A link: the token is the last path component of `/join/<token>`.
    if let url = URL(string: trimmed), url.scheme != nil {
        let last = url.lastPathComponent
        return last.isEmpty || last == "/" ? nil : last
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
    var origin = "\(scheme)://\(host)"
    if let port = url.port {
        origin += ":\(port)"
    }

    return try? ServerAddress.parse(origin, allowingCleartext: true).get()
}
