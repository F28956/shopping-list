import Testing
import Foundation
@testable import ShoppingList

/// The rule that decides what a list looks like, checked against the browser's.
struct GroupingTests {
    // Qualified: Swift Testing exports a `Tag` of its own for labelling tests, so the
    // bare name is ambiguous in here and nowhere else. Everything below infers the
    // type from this one signature.
    static func tag(
        _ id: Int64,
        _ name: String,
        order: Int64,
        emoji: String? = nil
    ) -> ShoppingList.Tag {
        let json = """
        {"id": \(id), "name": "\(name)", "sort_order": \(order),
         "emoji": \(emoji.map { "\"\($0)\"" } ?? "null")}
        """
        return try! JSONDecoder().decode(ShoppingList.Tag.self, from: Data(json.utf8))
    }

    static func item(_ id: Int64, _ name: String, tags: [Int64] = []) -> Item {
        let json = """
        {"id": \(id), "name": "\(name)", "amount": 1, "unit_id": null,
         "done_at": null, "tag_ids": \(tags)}
        """
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return try! decoder.decode(Item.self, from: Data(json.utf8))
    }

    static let produce = tag(1, "produce", order: 10)
    static let dairy = tag(2, "dairy", order: 40)
    static let frozen = tag(3, "frozen", order: 110, emoji: "🧊")
    static let tags = [dairy, frozen, produce]

    @Test func groupsFollowTheShopNotTheAlphabet() {
        let groups = grouped(
            [Self.item(1, "Milk", tags: [2]), Self.item(2, "Apples", tags: [1])],
            by: Self.tags
        )

        #expect(groups.map(\.heading) == ["produce", "dairy"], "sort_order, not A-Z")
    }

    @Test func anEmojiJoinsTheHeading() {
        let groups = grouped([Self.item(1, "Peas", tags: [3])], by: Self.tags)

        #expect(groups.map(\.heading) == ["🧊 frozen"])
    }

    /// The whole point of "first tag": one item, several categories, one place.
    @Test func severalTagsMeanTheEarliestOne() {
        let groups = grouped([Self.item(1, "Butter", tags: [2, 1])], by: Self.tags)

        #expect(groups.map(\.heading) == ["produce"], "produce is walked before dairy")
        #expect(groups.first?.items.count == 1, "and it appears once")
    }

    /// Decided from sort_order, not from the order the ids happen to arrive in --
    /// otherwise the rule breaks the day something sends them differently.
    @Test func theIdOrderDoesNotDecideIt() {
        let one = grouped([Self.item(1, "Butter", tags: [1, 2])], by: Self.tags)
        let other = grouped([Self.item(1, "Butter", tags: [2, 1])], by: Self.tags)

        // The headings, not the groups: the two items differ in the order of their
        // own tag ids, which is the input being varied rather than the outcome.
        #expect(one.map(\.heading) == other.map(\.heading))
        #expect(one.map(\.heading) == ["produce"])
    }

    @Test func untaggedFallsLast() {
        let groups = grouped(
            [Self.item(1, "Batteries"), Self.item(2, "Peas", tags: [3])],
            by: Self.tags
        )

        #expect(groups.map(\.heading) == ["🧊 frozen", "Other"])
    }

    /// A tag the client has never heard of -- deleted, or added since the tags were
    /// fetched -- must not lose the item.
    @Test func anUnknownTagDoesNotSwallowTheItem() {
        let groups = grouped([Self.item(1, "Mystery", tags: [99])], by: Self.tags)

        #expect(groups.map(\.heading) == ["Other"])
        #expect(groups.first?.items.first?.name == "Mystery")
    }

    /// Within a group the server's order stands: it is the answer about what is
    /// outstanding and what is done, and re-sorting would discard it.
    @Test func orderWithinAGroupIsKept() {
        let groups = grouped(
            [Self.item(1, "Yoghurt", tags: [2]), Self.item(2, "Milk", tags: [2])],
            by: Self.tags
        )

        #expect(groups.first?.items.map(\.name) == ["Yoghurt", "Milk"])
    }

    @Test func nothingGroupsToNothing() {
        #expect(grouped([], by: Self.tags).isEmpty)
    }
}
