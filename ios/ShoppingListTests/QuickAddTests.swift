import Testing
import Foundation
@testable import ShoppingList

/// The bridge to the Rust parser.
///
/// Not a second test suite for the parser -- that lives in `web/parsing`, has
/// forty-three cases, and is where a question about what `2 kg apples` means gets
/// settled. These are here to prove the crossing works: that a Swift string arrives
/// intact, that the answer comes back decoded, that the unit list is understood, and
/// that nothing leaks or crashes on the edges. Duplicating the parser's own cases here
/// would be maintaining the thing this whole exercise exists to stop maintaining twice.
struct QuickAddTests {
    private static let units = ["kg", "g", "litre", "ml", "pint", "fl oz", "unit"]

    @Test("a line with an amount and a unit comes back in three pieces")
    func splitsALine() {
        let parsed = QuickAdd.parse("2 kg apples", units: Self.units)
        #expect(parsed.name == "apples")
        #expect(parsed.amount == 2)
        #expect(parsed.unit == "kg")
    }

    @Test("a bare name keeps its amount of one and names no unit")
    func bareName() {
        let parsed = QuickAdd.parse("Sourdough", units: Self.units)
        #expect(parsed.name == "Sourdough")
        #expect(parsed.amount == 1)
        #expect(parsed.unit == nil)
    }

    @Test("a multi-word unit is matched whole")
    func multiWordUnit() {
        // `fl oz` and not `fl`, which is what the longest-first matching is for. It is
        // the case most likely to break if the boundary ever mangled the unit list.
        let parsed = QuickAdd.parse("6 fl oz cream", units: Self.units)
        #expect(parsed.unit == "fl oz")
        #expect(parsed.name == "cream")
    }

    @Test("a unit that was not offered is left in the name")
    func unknownUnit() {
        // Proves the list crossed over and was actually consulted, rather than the
        // parser having a built-in idea of what a unit is.
        let parsed = QuickAdd.parse("2 furlongs rope", units: Self.units)
        #expect(parsed.name == "furlongs rope")
        #expect(parsed.amount == 2)
        #expect(parsed.unit == nil)
    }

    @Test("no units at all is not a crash")
    func noUnits() {
        // The standalone case before the reference data has loaded, and an empty JSON
        // array over the boundary.
        let parsed = QuickAdd.parse("2 kg apples", units: [])
        #expect(parsed.amount == 2)
        #expect(parsed.unit == nil)
    }

    @Test("an empty line is an empty answer rather than a crash")
    func empty() {
        let parsed = QuickAdd.parse("", units: Self.units)
        #expect(parsed.name == "")
        #expect(parsed.amount == 1)
    }

    @Test("names that are not ASCII survive the crossing")
    func unicode() {
        // The boundary is C strings, which is where a UTF-8 mistake would show up:
        // Rust would refuse the line and Swift would be handed back nothing.
        let parsed = QuickAdd.parse("3 kg bulvės 🥔", units: Self.units)
        #expect(parsed.name == "bulvės 🥔")
        #expect(parsed.amount == 3)
        #expect(parsed.unit == "kg")
    }

    @Test("a fractional amount keeps its fraction")
    func fractional() {
        let parsed = QuickAdd.parse("0.5 kg flour", units: Self.units)
        #expect(parsed.amount == 0.5)
        #expect(parsed.name == "flour")
    }

    @Test("parsing many lines does not run out of memory")
    func repeated() {
        // The free is in a `defer`, and a `defer` that was ever wrong would show here
        // rather than in a crash months later. Cheap enough to keep.
        for n in 0..<5_000 {
            _ = QuickAdd.parse("\(n) kg apples", units: Self.units)
        }
    }
}
