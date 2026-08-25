import Testing
import Foundation
@testable import ShoppingList

/// The editor's rules, which are the ones that decide whether the phone sends the
/// server something it will refuse.
struct ItemDraftTests {
    static func item(name: String = "Apples", amount: Double = 2, unitID: Int64? = 19) -> Item {
        let json = """
        {"id": 7, "name": "\(name)", "amount": \(amount),
         "unit_id": \(unitID.map(String.init) ?? "null"), "done_at": null}
        """
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return try! decoder.decode(Item.self, from: Data(json.utf8))
    }

    @Test func aDraftStartsAsTheItem() {
        let draft = ItemDraft(item: Self.item())

        #expect(draft.name == "Apples")
        #expect(draft.amount == "2", "a whole amount loses its .0")
        #expect(draft.unitID == 19)
    }

    @Test func afractionalAmountKeepsItsPoint() {
        #expect(ItemDraft(item: Self.item(amount: 1.5)).amount == "1.5")
    }

    @Test func aValidDraftGivesBackWhatToSend() throws {
        let typed = try #require(ItemDraft(item: Self.item()).validated)

        #expect(typed.name == "Apples")
        #expect(typed.amount == 2)
        #expect(typed.unitID == 19)
    }

    @Test func nameIsTrimmed() throws {
        var draft = ItemDraft(item: Self.item())
        draft.name = "  Braeburn apples \n"

        #expect(try #require(draft.validated).name == "Braeburn apples")
    }

    /// The decimal pad offers whichever separator the phone is set to, and `Double(_:)`
    /// only reads a full stop.
    @Test func aCommaIsADecimalPoint() throws {
        var draft = ItemDraft(item: Self.item())
        draft.amount = "1,5"

        #expect(try #require(draft.validated).amount == 1.5)
    }

    @Test func clearingTheUnitIsAllowed() throws {
        var draft = ItemDraft(item: Self.item())
        draft.unitID = nil

        #expect(try #require(draft.validated).unitID == nil)
    }

    /// Each of these is something the server answers 400 to, so Save must not offer.
    @Test(arguments: [
        ("", "2", "an empty name"),
        ("   ", "2", "a name that is only spaces"),
        ("Apples", "", "no amount at all"),
        ("Apples", "0", "nothing of it"),
        ("Apples", "-1", "less than nothing"),
        ("Apples", "two", "an amount that is not a number"),
        ("Apples", "inf", "an amount with no end"),
    ])
    func unsaveableDrafts(name: String, amount: String, why: String) {
        var draft = ItemDraft(item: Self.item())
        draft.name = name
        draft.amount = amount

        #expect(draft.validated == nil, "\(why) cannot be saved")
    }

    /// `Int(_:)` traps on these, and the amount arrives from the network.
    @Test(arguments: [Double.nan, .infinity, 1e300])
    func anImpossibleAmountDoesNotCrash(_ amount: Double) {
        #expect(!amount.asAmount.isEmpty)
    }
}
