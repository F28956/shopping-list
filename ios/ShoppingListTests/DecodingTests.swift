import Testing
import Foundation
@testable import ShoppingList

/// What the API actually sends, checked against what this app expects it to send.
///
/// The sign-in screens cannot be reached without a client id, but this layer can: it
/// is where a phone and a browser come to disagree about what is on a list, and it is
/// the half that has no excuse for being unverified.
struct DecodingTests {
    /// Copied from a real response, snake_case and RFC 3339 timestamps included.
    static let itemJSON = """
    {
        "id": 7,
        "list_id": 2,
        "name": "Apples",
        "amount": 1.5,
        "unit_id": 19,
        "done_at": "2026-08-25T09:41:02Z",
        "created_at": "2026-08-24T18:03:11Z"
    }
    """

    static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }

    @Test func anItemDecodes() throws {
        let item = try Self.decoder().decode(Item.self, from: Data(Self.itemJSON.utf8))

        #expect(item.id == 7)
        #expect(item.name == "Apples")
        #expect(item.amount == 1.5)
        #expect(item.unitID == 19)
        #expect(item.isDone, "a done_at means done")
    }

    /// The identity a queued operation will name a row by. Two tests in one, because
    /// the two halves fail differently: a server that sends it must be read, and a
    /// server that predates it must not fail the decode -- there is nothing queued
    /// against a row yet, so an absent uuid is a poorer row rather than a broken one.
    @Test func aUuidIsReadWhenSentAndEmptyWhenNot() throws {
        let withOne = """
        { "id": 1, "uuid": "a1b2c3d4-e5f6-4789-a012-3456789abcde", "list_id": 1,
          "name": "Bread", "amount": 1, "unit_id": null, "done_at": null,
          "created_at": "2026-08-24T18:03:11Z" }
        """

        let named = try Self.decoder().decode(Item.self, from: Data(withOne.utf8))
        let unnamed = try Self.decoder().decode(Item.self, from: Data(Self.itemJSON.utf8))

        #expect(named.uuid == "a1b2c3d4-e5f6-4789-a012-3456789abcde")
        #expect(unnamed.uuid == "")
    }

    /// The nullable columns are the ones that go wrong: an outstanding item has no
    /// done_at, and an unmeasured one has no unit.
    @Test func theNullsSurvive() throws {
        let json = """
        { "id": 1, "list_id": 1, "name": "Bread", "amount": 1,
          "unit_id": null, "done_at": null, "created_at": "2026-08-24T18:03:11Z" }
        """

        let item = try Self.decoder().decode(Item.self, from: Data(json.utf8))

        #expect(item.unitID == nil)
        #expect(item.doneAt == nil)
        #expect(!item.isDone)
    }

    @Test func aPageDecodes() throws {
        let json = """
        { "items": [\(Self.itemJSON)], "total": 12, "total_pages": 1, "has_more": false }
        """

        let page = try Self.decoder().decode(Page<Item>.self, from: Data(json.utf8))

        #expect(page.items.count == 1)
        #expect(page.total == 12)
        #expect(!page.hasMore)
    }

    /// A field this app does not use must not break it — the API is allowed to grow.
    @Test func anUnknownFieldIsIgnored() throws {
        let json = """
        { "id": 3, "name": "Dairy", "owner_id": 1, "something_new": "later" }
        """

        let list = try Self.decoder().decode(List.self, from: Data(json.utf8))

        #expect(list.name == "Dairy")
    }
}

/// The one rule this app implements rather than asks the server for, so it is the one
/// that can drift from the web UI.
struct MeasureTests {
    private func item(amount: Double, unit: Int64?) -> Item {
        let json = """
        { "id": 1, "list_id": 1, "name": "x", "amount": \(amount),
          "unit_id": \(unit.map(String.init) ?? "null"), "done_at": null,
          "created_at": "2026-08-24T18:03:11Z" }
        """
        return try! DecodingTests.decoder().decode(Item.self, from: Data(json.utf8))
    }

    private let units: [Int64: String] = [19: "kg", 4: "pint"]

    /// One of something unmeasured says nothing, because "1" is not information.
    @Test func oneOfSomethingUnmeasuredShowsNothing() {
        #expect(item(amount: 1, unit: nil).measure(units: units) == nil)
    }

    @Test func aBareCountShowsTheCount() {
        #expect(item(amount: 6, unit: nil).measure(units: units) == "6")
    }

    @Test func aMeasureShowsBoth() {
        #expect(item(amount: 2, unit: 19).measure(units: units) == "2 kg")
    }

    /// Whole numbers lose the decimal point; 1.5 keeps it.
    @Test func fractionsSurviveAndWholeNumbersDoNotGrow() {
        #expect(item(amount: 1.5, unit: 19).measure(units: units) == "1.5 kg")
        #expect(item(amount: 4, unit: 4).measure(units: units) == "4 pint")
    }

    /// One of something measured still says which unit — it is "1 pint", not nothing.
    @Test func oneOfSomethingMeasuredStillSaysSo() {
        #expect(item(amount: 1, unit: 4).measure(units: units) == "1 pint")
    }

    /// A unit the server knows and this app has not loaded must not print an id.
    @Test func anUnknownUnitIsOmittedRatherThanGuessed() {
        #expect(item(amount: 2, unit: 999).measure(units: units) == "2")
    }
}
