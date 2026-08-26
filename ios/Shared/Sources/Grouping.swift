import Foundation

/// A heading and the items under it.
struct ItemGroup: Identifiable, Equatable {
    let heading: String
    let items: [Item]

    var id: String { heading }
}

/// The items under their category heading, in the order this list is walked.
///
/// `tags` arrives already in that order — resolved by the service, per person, per
/// list — so this reads position in that array and nothing else. It used to read
/// `sortOrder`, which is one global opinion: it put every shop-name tag last and
/// could never let `urgent` lead.
///
/// An item with several tags falls under whichever of them comes first here, and an
/// untagged one falls under "Other", last.
func grouped(_ items: [Item], by tags: [Tag]) -> [ItemGroup] {
    let placed = Dictionary(uniqueKeysWithValues: tags.enumerated().map { ($0.element.id, $0.offset) })

    /// Where this item sits, and what to call the group.
    func category(_ item: Item) -> (order: Int, heading: String) {
        let primary = item.tagIDs
            .compactMap { id in placed[id].map { (at: $0, tag: tags[$0]) } }
            .min { $0.at < $1.at }

        guard let primary else { return (.max, "Other") }

        guard let emoji = primary.tag.emoji, !emoji.isEmpty else {
            return (primary.at, primary.tag.name)
        }
        return (primary.at, "\(emoji) \(primary.tag.name)")
    }

    // Built in encounter order and sorted at the end, so items keep the order they
    // arrived in within their group -- that order is the server's answer about what
    // is outstanding and what is done, and re-sorting it here would discard it.
    var order: [String: Int] = [:]
    var members: [String: [Item]] = [:]
    var seen: [String] = []

    for item in items {
        let (position, heading) = category(item)
        if members[heading] == nil {
            seen.append(heading)
            order[heading] = position
        }
        members[heading, default: []].append(item)
    }

    return seen
        .sorted { (order[$0]!, $0) < (order[$1]!, $1) }
        .map { ItemGroup(heading: $0, items: members[$0]!) }
}
