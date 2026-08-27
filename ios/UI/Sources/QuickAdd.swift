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

    /// The remembered names worth offering for something part-typed, best first.
    ///
    /// The store is the device's own — it is that person's shopping and there is
    /// nowhere else for it to live — but the **policy is not**. Which of "milk" and
    /// "milk chocolate" comes first when you have typed `mil` is a judgement about
    /// how often and how recently each was bought, and a judgement made twice is a
    /// judgement that will differ. This is the server's `fuzzy` and its
    /// `history_rank`, compiled in.
    static func suggest(
        _ query: String,
        from remembered: [Cache.Remembered],
        now: Date = Date()
    ) -> [String] {
        let candidates = remembered.map {
            ["name": $0.name, "uses": $0.uses, "last_used_at": $0.lastUsedAt] as [String: Any]
        }
        let input: [String: Any] = [
            "query": query,
            // Passed rather than read across the boundary: the Rust side has no clock,
            // which is what lets a test say what "recently" means.
            "now": Int64(now.timeIntervalSince1970),
            "candidates": candidates,
        ]

        guard let json = try? JSONSerialization.data(withJSONObject: input),
              let text = String(data: json, encoding: .utf8)
        else { return [] }

        guard let answer = quickadd_suggest(text) else { return [] }
        defer { quickadd_free(answer) }

        let data = Data(String(cString: answer).utf8)
        let decoded = try? JSONDecoder().decode(Suggested.self, from: data)
        return decoded?.names ?? []
    }

    private struct Suggested: Decodable {
        var names: [String]
    }

    /// What a typed line should do to a list.
    ///
    /// The whole decision, and deliberately not this app's to make. Which unit a bare
    /// name ends up in, whether `Milk` is the `milk` already on the list, and whether
    /// a crossed-off row comes back are all rules the server has always had — and
    /// writing them out again here is how a phone came to show three rows where a
    /// server would have shown one. See `parsing::add`.
    static func resolve(
        _ line: String,
        units: [Unit],
        rows: [Item],
        remembered: Cache.Remembered?
    ) -> Resolution {
        var input: [String: Any] = [
            "line": line,
            "units": units.map { ["id": $0.id, "name": $0.name] },
            "rows": rows.map {
                [
                    "uuid": $0.uuid,
                    "name": $0.name,
                    "unit_id": $0.unitID as Any,
                    "done": $0.isDone,
                ]
            },
        ]
        if let remembered {
            input["remembered"] = [
                "unit_id": remembered.unitID as Any,
                "amount": remembered.amount as Any,
                "tag_ids": remembered.tagIDs,
            ]
        }

        let fallback = Resolution.new(
            .init(name: line, amount: 1, unitID: nil, tagIDs: [])
        )

        guard let json = try? JSONSerialization.data(withJSONObject: input),
              let text = String(data: json, encoding: .utf8),
              let answer = quickadd_resolve(text)
        else { return fallback }
        defer { quickadd_free(answer) }

        let data = Data(String(cString: answer).utf8)
        guard let decoded = try? JSONDecoder().decode(Answer.self, from: data) else {
            return fallback
        }
        if let existing = decoded.existing {
            return .existing(uuid: existing.uuid, putBack: existing.putBack)
        }
        return decoded.new.map(Resolution.new) ?? fallback
    }

    enum Resolution {
        /// The list already wants this. `putBack` when the row was crossed off.
        case existing(uuid: String, putBack: Bool)
        case new(NewRow)
    }

    struct NewRow: Decodable {
        var name: String
        var amount: Double
        var unitID: Int64?
        var tagIDs: [Int64]

        enum CodingKeys: String, CodingKey {
            case name, amount
            case unitID = "unit_id"
            case tagIDs = "tag_ids"
        }
    }

    private struct Answer: Decodable {
        var existing: ExistingRow?
        var new: NewRow?

        struct ExistingRow: Decodable {
            var uuid: String
            var putBack: Bool

            enum CodingKeys: String, CodingKey {
                case uuid
                case putBack = "put_back"
            }
        }
    }
}
