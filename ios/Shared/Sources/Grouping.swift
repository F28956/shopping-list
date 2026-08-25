import Foundation

/// A heading and the items under it.
struct ItemGroup: Identifiable, Equatable {
    let heading: String
    let items: [Item]

    var id: String { heading }
}

/// The items under their category heading, in the order the shop is laid out.
///
/// The same rule the browser follows, kept here so the three screens agree: an item
/// with several tags falls under the one that comes first in the shop, and an untagged
/// one falls under "Other", last whatever the tags are numbered.
///
/// "First" is decided from `sortOrder` rather than from the order the ids arrive in.
/// The server does send them sorted, but a rule that depends on that is a rule that
/// breaks silently the day something else answers.
func grouped(_ items: [Item], by tags: [Tag]) -> [ItemGroup] {
    let byID = Dictionary(uniqueKeysWithValues: tags.map { ($0.id, $0) })

    /// Where this item sits, and what to call the group.
    func category(_ item: Item) -> (order: Int64, heading: String) {
        let primary = item.tagIDs
            .compactMap { byID[$0] }
            .min { ($0.sortOrder, $0.name) < ($1.sortOrder, $1.name) }

        guard let primary else { return (.max, "Other") }

        guard let emoji = primary.emoji, !emoji.isEmpty else {
            return (primary.sortOrder, primary.name)
        }
        return (primary.sortOrder, "\(emoji) \(primary.name)")
    }

    // Built in encounter order and sorted at the end, so items keep the order they
    // arrived in within their group -- that order is the server's answer about what
    // is outstanding and what is done, and re-sorting it here would discard it.
    var order: [String: Int64] = [:]
    var members: [String: [Item]] = [:]
    var seen: [String] = []

    for item in items {
        let (sortOrder, heading) = category(item)
        if members[heading] == nil {
            seen.append(heading)
            order[heading] = sortOrder
        }
        members[heading, default: []].append(item)
    }

    return seen
        .sorted { (order[$0]!, $0) < (order[$1]!, $1) }
        .map { ItemGroup(heading: $0, items: members[$0]!) }
}
