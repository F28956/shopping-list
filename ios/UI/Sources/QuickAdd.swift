import Foundation
import QuickAddC

/// What `2 kg apples` means.
///
/// One parser, shared with the server, because the alternative was three of them. The
/// same line typed into this app, the Android app and the web page has to produce the
/// same item — and "has to" is not something three separate implementations of a
/// hundred lines of unit matching and number parsing were ever going to manage. The
/// forty-three cases it is tested against live in `web/parsing/src/quick_add.rs`, and
/// they are the server's tests, not a copy of them.
///
/// Absent from the watch, deliberately: it has no keyboard and nothing to parse.
enum QuickAdd {
    /// The reading of `line`, with `units` as the unit names that exist.
    ///
    /// Never fails. A line it cannot make sense of comes back whole as the name, with
    /// an amount of one — which is what somebody typing a shopping list means by a
    /// line the computer did not understand.
    static func parse(_ line: String, units: [String]) -> Parsed {
        // The unit names go over as JSON because the boundary is C and an array of
        // strings is not a C type. Encoding cannot fail for `[String]`.
        let unitsJSON = (try? JSONEncoder().encode(units))
            .flatMap { String(data: $0, encoding: .utf8) } ?? "[]"

        guard let answer = quickadd_parse(line, unitsJSON) else {
            return Parsed(name: line, amount: 1, unit: nil)
        }
        // Freed on every path out, including the failed-decode one below.
        defer { quickadd_free(answer) }

        let json = Data(String(cString: answer).utf8)
        guard let parsed = try? JSONDecoder().decode(Parsed.self, from: json) else {
            return Parsed(name: line, amount: 1, unit: nil)
        }
        return parsed
    }

    struct Parsed: Decodable, Equatable {
        var name: String
        var amount: Double
        /// The unit the line named, in the form it was named in, or `nil`.
        var unit: String?
    }
}
