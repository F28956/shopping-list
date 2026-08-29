import Foundation

/// A class of request, with the numbers taken out.
///
/// What a log line and a metric label are allowed to say about *which* request this
/// was. The path itself is not: `/api/lists/41/items` names a list, and once list ids
/// are labels a collector has one series per list and a cardinality problem that grows
/// with somebody's shopping. A share link's token and an invite's code travel in paths
/// too, and neither belongs anywhere near a metric.
///
/// So a path is folded down to one of these before anything is written. Unrecognised
/// paths become ``other`` rather than being passed through, which is the difference
/// between a new route being uninteresting and a new route quietly leaking its
/// arguments.
enum Route: String, Sendable, CaseIterable {
    case lists
    case listItems
    case listTags
    case listPeople
    case listInvites
    case listEvents
    case itemTags
    case units
    case tags
    case suggestions
    case history
    case sync
    case myEvents
    case me
    case admissions
    case server
    case join
    case other

    /// Folds a path down to its class.
    ///
    /// Matching on the shape rather than on a prefix: `/api/lists` and
    /// `/api/lists/41/items` are different questions with different costs, and telling
    /// them apart is most of what makes a latency histogram worth reading.
    init(path: String) {
        // The query string is arguments, and one of them is what somebody is typing into
        // the add field -- `history?q=`. Dropped before anything looks at it.
        let withoutQuery = path.split(separator: "?", maxSplits: 1).first.map(String.init) ?? path
        let parts = Array(
            withoutQuery
                .split(separator: "/")
                .map(String.init)
                .drop(while: { $0 == "api" })
        )

        // An email in a path. `/api/admissions/{email}` is the one route that puts a
        // person's address in the URL, and it is the reason this is a fold rather than a
        // sanitiser: whatever is not recognised below becomes `other`, so a route that is
        // forgotten here loses its arguments rather than publishing them.
        if parts.first == "admissions" { self = .admissions; return }

        // Every id replaced by a star before anything is matched. Ids are numbers on this
        // API, so this is exact -- and a segment that is *not* a number and not a word
        // this file knows is left alone, which lands the whole path in `other`.
        let shape = parts.map { part in
            part.allSatisfy(\.isNumber) && !part.isEmpty ? "*" : part
        }.joined(separator: "/")

        switch shape {
        case "lists", "lists/*": self = .lists
        case "lists/*/items", "lists/*/items/*", "lists/*/items/done",
             "lists/*/items/*/done":
            self = .listItems
        case "lists/*/items/*/tags", "lists/*/items/*/tags/*": self = .itemTags
        case "lists/*/tag-order": self = .listTags
        case "lists/*/members", "lists/*/members/*": self = .listPeople
        case "lists/*/members/invites": self = .listInvites
        case "lists/*/events": self = .listEvents
        // Two different questions on one prefix, and told apart because their costs are
        // nothing alike: `history?q=` is the lookup behind every keystroke in the add
        // field, `history/entries` is the whole of what this list has ever held.
        case "lists/*/history": self = .suggestions
        case "lists/*/history/entries": self = .history
        case "units": self = .units
        case "tags", "tags/*": self = .tags
        case "sync": self = .sync
        case "me/events": self = .myEvents
        case "me": self = .me
        case "server": self = .server
        case "invites": self = .join
        default: self = .other
        }
    }
}

/// What a request came back as, in five buckets.
///
/// Not the status code. A code is a low-cardinality label right up until a server
/// starts answering 418s, and the question anybody actually asks of a dashboard is
/// which of these five it was.
enum RequestOutcome: String, Sendable {
    case ok
    /// The far end was not reached at all: no signal, no server, TLS refused.
    case unreachable
    /// 401 or 403 — the request arrived and was turned away.
    case refused
    /// 400, 404, 409, 422 — the request arrived and was wrong.
    case rejected
    /// 5xx, or an answer that could not be read.
    case failed

    init(status: Int) {
        switch status {
        case 200..<300: self = .ok
        case 401, 403: self = .refused
        case 400, 404, 409, 422: self = .rejected
        default: self = .failed
        }
    }

    init(error: Error) {
        switch Plain.Failure(error) {
        case .transport: self = .unreachable
        case .unauthorized, .forbidden, .notAdmitted: self = .refused
        case .notFound, .badInput: self = .rejected
        default: self = .failed
        }
    }
}
