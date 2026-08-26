import Testing
import Foundation
@testable import ShoppingList

/// What the apps were getting wrong about what the server told them.
struct ListingTests {
    static func decode<T: Decodable>(_ json: String) throws -> T {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(T.self, from: Data(json.utf8))
    }

    @Test func aListCarriesWhatYouMayDoWithIt() throws {
        let list: ShoppingList.List = try Self.decode(
            #"{"id": 1, "name": "Home", "owner_id": 4, "role": "editor"}"#
        )

        #expect(list.role == .editor)
        #expect(list.mayEdit)
    }

    /// A server that did not say gets the safe reading, not the generous one: a list
    /// shown read-only is a smaller mistake than one offering controls that refuse.
    @Test func aListWithNoRoleIsReadOnly() throws {
        let list: ShoppingList.List = try Self.decode(
            #"{"id": 1, "name": "Home", "owner_id": 4}"#
        )

        #expect(list.role == .viewer)
        #expect(!list.mayEdit)
    }

    /// `held >= needed`, the way the service's own checks read.
    @Test func rolesAreOrdered() {
        #expect(Role.viewer < Role.editor)
        #expect(Role.editor < Role.owner)
        #expect(Role.owner >= Role.editor)
        #expect(!(Role.viewer >= Role.editor))
    }

    /// The flag that was decoded and never read. A prefix presented as the whole list
    /// makes the rows that did not fit look deleted.
    @Test func aTruncatedPageSaysSo() throws {
        let page: Page<ShoppingList.Unit> = try Self.decode(
            #"{"items": [{"id": 1, "name": "kg"}], "total": 340, "has_more": true}"#
        )
        let listing = Listing(page)

        #expect(listing.truncated)
        #expect(listing.total == 340)
        #expect(listing.items.count == 1)
    }

    @Test func aCompletePageDoesNot() throws {
        let page: Page<ShoppingList.Unit> = try Self.decode(
            #"{"items": [{"id": 1, "name": "kg"}], "total": 1, "has_more": false}"#
        )

        #expect(!Listing(page).truncated)
    }

    /// An item on the list route carries its tags; one from a single-item route does
    /// not, and that is not a decoding failure.
    @Test func anItemWithoutTagsStillDecodes() throws {
        let item: Item = try Self.decode(
            #"{"id": 7, "name": "Apples", "amount": 1, "unit_id": null, "done_at": null}"#
        )

        #expect(item.tagIDs.isEmpty)
    }

    /// `measure` used to keep its own copy of this rule, without the guard.
    @Test func measureUsesTheSharedAmountRule() throws {
        let item: Item = try Self.decode(
            #"{"id": 7, "name": "Apples", "amount": 2, "unit_id": 3, "done_at": null}"#
        )

        #expect(item.measure(units: [3: "kg"]) == "2 kg", "not 2.0 kg")
    }
}
