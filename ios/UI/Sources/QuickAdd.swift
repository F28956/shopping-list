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
        let ask = AskSuggestions(
            query: query,
            // Passed rather than read across the boundary: the Rust side has no clock,
            // which is what lets a test say what "recently" means.
            now: Int64(now.timeIntervalSince1970),
            candidates: remembered.map {
                AskSuggestions.Candidate(
                    name: $0.name,
                    uses: $0.uses,
                    lastUsedAt: $0.lastUsedAt
                )
            }
        )

        guard let answer = call(quickadd_suggest, with: ask),
              let decoded = try? JSONDecoder().decode(Suggested.self, from: answer)
        else { return [] }
        return decoded.names
    }

    /// Typed for the same reason as `Ask` — see there.
    private struct AskSuggestions: Encodable {
        var query: String
        var now: Int64
        var candidates: [Candidate]

        struct Candidate: Encodable {
            var name: String
            var uses: Int64
            var lastUsedAt: Int64

            enum CodingKeys: String, CodingKey {
                case name, uses
                case lastUsedAt = "last_used_at"
            }
        }
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
        let ask = Ask(
            line: line,
            units: units.map { Ask.Unit(id: $0.id, name: $0.name, bare: $0.bare) },
            rows: rows.map {
                Ask.Row(uuid: $0.uuid, name: $0.name, unitID: $0.unitID, done: $0.isDone)
            },
            remembered: remembered.map {
                Ask.Remembered(unitID: $0.unitID, amount: $0.amount, tagIDs: $0.tagIDs)
            }
        )

        let fallback = Resolution.new(
            .init(name: line, amount: 1, unitID: nil, tagIDs: [])
        )

        guard let answer = call(quickadd_resolve, with: ask) else { return fallback }
        guard let decoded = try? JSONDecoder().decode(Answer.self, from: answer) else {
            return fallback
        }
        if let existing = decoded.existing {
            return .existing(uuid: existing.uuid, putBack: existing.putBack)
        }
        return decoded.new.map(Resolution.new) ?? fallback
    }

    /// What the shared rules are given, spelled out as types.
    ///
    /// A dictionary was the obvious thing and was the wrong one. Leaving a key out of
    /// `["id": …, "name": …]` compiles, and the Rust side used to fill the gap with a
    /// default — so `bare` went missing and every unit came back unable to stand
    /// alone, which is a right rule reaching a wrong answer from a wrong input. These
    /// are the same fields `parsing::add` reads, and a member left out will not build.
    ///
    /// They mirror Rust by hand, which is the part a compiler cannot check. What backs
    /// it up is the other end: nothing here is `#[serde(default)]` any more, so a
    /// mirror that drifts fails to decode loudly rather than answering plausibly.
    private struct Ask: Encodable {
        var line: String
        var units: [Unit]
        var rows: [Row]
        var remembered: Remembered?

        struct Unit: Encodable {
            var id: Int64
            var name: String
            var bare: Bool
        }

        struct Row: Encodable {
            var uuid: String
            var name: String
            var unitID: Int64?
            var done: Bool

            enum CodingKeys: String, CodingKey {
                case uuid, name, done
                case unitID = "unit_id"
            }
        }

        struct Remembered: Encodable {
            var unitID: Int64?
            var amount: Double?
            var tagIDs: [Int64]

            enum CodingKeys: String, CodingKey {
                case amount
                case unitID = "unit_id"
                case tagIDs = "tag_ids"
            }
        }
    }

    /// Hands one of the C entry points a question and takes back its answer.
    ///
    /// The free is here rather than at each call site, which is the only place it
    /// could be forgotten.
    private static func call(
        _ entry: (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>?,
        with body: some Encodable
    ) -> Data? {
        guard let json = try? JSONEncoder().encode(body),
              let text = String(data: json, encoding: .utf8),
              let answer = entry(text)
        else { return nil }
        defer { quickadd_free(answer) }
        return Data(String(cString: answer).utf8)
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
