import Foundation

/// A heading and the items under it.
struct ItemGroup: Identifiable, Equatable {
    let heading: String
    let items: [Item]

    var id: String { heading }
}

/// The tag an item is filed under: the first of its tags in this list's order.
///
/// The one that decides where the item sits, which is why a screen showing a single
/// tag beside an item should show this one — any other would name a place the item is
/// not.
///
/// `tags` must be in the list's order, as the service resolves it.
func primaryTag(of item: Item, in tags: [Tag]) -> Tag? {
    let placed = Dictionary(
        uniqueKeysWithValues: tags.enumerated().map { ($0.element.id, $0.offset) }
    )

    return item.tagIDs
        .compactMap { id in placed[id].map { (at: $0, tag: tags[$0]) } }
        .min { $0.at < $1.at }?
        .tag
}

extension Tag {
    /// The tag as a heading or a chip: its emoji, when it has one, then its name.
    var heading: String {
        guard let emoji, !emoji.isEmpty else { return name }
        return "\(emoji) \(name)"
    }
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
        guard let primary = primaryTag(of: item, in: tags), let at = placed[primary.id] else {
            return (.max, "Other")
        }
        return (at, primary.heading)
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
