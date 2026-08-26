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
