#if DEBUG

    import Foundation

    /// A server, in memory, for UI tests.
    ///
    /// Answers the same JSON the real one does — the app's decoding, grouping and
    /// display all run for real against it — and mutates in place, so a test can tick
    /// something off and then assert on what comes back.
    ///
    /// Deliberately not a mock framework: what a test wants to say is "given this
    /// list, when I do that, the screen says this", and a small mutable world says it
    /// better than a pile of expectations.
    final class StubWorld: @unchecked Sendable {
        static let shared = StubWorld()

        struct Row {
            var id: Int64
            /// Which list it is on. Without this the world answered the same rows for
            /// every list, and a brand new list arrived with somebody else's shopping
            /// already on it.
            var listID: Int64 = 1
            var name: String
            var amount: Double
            var unitID: Int64?
            var doneAt: String?
            var tagIDs: [Int64]
        }

        struct StubList {
            var id: Int64
            var name: String
            var role: String
        }

        private let lock = NSLock()
        private var rows: [Row] = []
        private var lists: [StubList] = []
        /// This person's tag order for a list, as the service resolves it: what they
        /// placed leads, everything else keeps the shop's order behind it.
        private var placed: [Int64: [Int64]] = [:]
        private(set) var itemsTruncated = false
        private(set) var itemsTotal: Int64 = 0

        /// Units and tags are reference data on the real server too — seeded by
        /// migration, ordered by where they fall in a shop.
        /// The third is `bare`: whether it may be written with no number in front of
        /// it. `kg` and `pint` may; `unit` may not, being a word that starts names.
        let units: [(Int64, String, Bool)] = [
            (1, "kg", true), (2, "unit", false), (3, "pint", true),
        ]
        let tags: [(Int64, String, Int64, String?)] = [
            (10, "produce", 10, nil),
            (20, "fruits", 20, "🍎"),
            (30, "bakery", 30, nil),
            (40, "dairy", 40, nil),
            // On nothing, deliberately: a tag in the order that no item carries is
            // the case that reads as the ordering being broken.
            (50, "frozen", 110, "🧊"),
        ]

        func reset(scenario: String) {
            lock.lock()
            defer { lock.unlock() }

            lists = [StubList(id: 1, name: "Home", role: scenario == "viewer" ? "viewer" : "owner")]
            placed = [:]
            itemsTruncated = scenario == "truncated"
            rows = [
                Row(id: 1, name: "Milk", amount: 1, unitID: 3, doneAt: nil, tagIDs: [40]),
                Row(id: 2, name: "Apples", amount: 2, unitID: 1, doneAt: nil, tagIDs: [20]),
                Row(id: 3, name: "Bread", amount: 1, unitID: 2, doneAt: nil, tagIDs: [30]),
                Row(id: 4, name: "Batteries", amount: 1, unitID: nil, doneAt: nil, tagIDs: []),
                Row(
                    id: 5, name: "Potatoes", amount: 1, unitID: 1,
                    doneAt: "2026-08-26T09:00:00Z", tagIDs: [10]
                ),
            ]
            itemsTotal = itemsTruncated ? 340 : Int64(rows.count)
        }

        // MARK: - Reading

        func listsJSON() -> String {
            lock.lock()
            defer { lock.unlock() }
            let items = lists.map {
                #"{"id": \#($0.id), "name": "\#($0.name)", "owner_id": 1, "role": "\#($0.role)"}"#
            }
            return """
            {"items": [\(items.joined(separator: ","))], "total": \(lists.count),
             "total_pages": 1, "has_more": false}
            """
        }

        /// What this list has bought before.
        ///
        /// The real matching is loose and ranked, and lives in the service; this is
        /// the smallest thing that behaves like it for a test — same shape, same cap,
        /// same rule that what you have already typed in full is not a suggestion.
        func historyJSON(matching query: String) -> String {
            let remembered = [
                "Milk", "Milk chocolate", "Milled oats", "Bread", "Bread rolls",
                "Butter", "Bananas", "Batteries",
            ]
            let wanted = query.trimmingCharacters(in: .whitespaces).lowercased()
            guard !wanted.isEmpty else { return "[]" }

            let hits = remembered
                .filter { $0.lowercased().hasPrefix(wanted) && $0.lowercased() != wanted }
                .prefix(6)
                .map { #""\#($0)""# }
            return "[\(hits.joined(separator: ","))]"
        }

        // MARK: - Managing lists

        /// Returns the new list, because the app selects what it just made.
        func createList(named name: String) -> String {
            lock.lock()
            defer { lock.unlock() }
            let next = (lists.map(\.id).max() ?? 0) + 1
            lists.append(StubList(id: next, name: name, role: "owner"))
            return #"{"id": \#(next), "name": "\#(name)", "owner_id": 1, "role": "owner"}"#
        }

        func renameList(_ id: Int64, to name: String) {
            lock.lock()
            defer { lock.unlock() }
            guard let at = lists.firstIndex(where: { $0.id == id }) else { return }
            lists[at].name = name
        }

        func deleteList(_ id: Int64) {
            lock.lock()
            defer { lock.unlock() }
            lists.removeAll { $0.id == id }
            // Items belong to the list, and the real server cascades.
            rows.removeAll { $0.listID == id }
        }

        func itemsJSON(list: Int64) -> String {
            lock.lock()
            defer { lock.unlock() }
            let mine = rows.filter { $0.listID == list }
            // Outstanding first, then done — the order the real route is asked for.
            let ordered = mine.filter { $0.doneAt == nil } + mine.filter { $0.doneAt != nil }
            let items = ordered.map { row in
                """
                {"id": \(row.id), "name": "\(row.name)", "amount": \(row.amount),
                 "unit_id": \(row.unitID.map(String.init) ?? "null"),
                 "done_at": \(row.doneAt.map { "\"\($0)\"" } ?? "null"),
                 "tag_ids": \(row.tagIDs)}
                """
            }
            let total = itemsTruncated ? itemsTotal : Int64(mine.count)
            return """
            {"items": [\(items.joined(separator: ","))], "total": \(total),
             "total_pages": 1, "has_more": \(itemsTruncated)}
            """
        }

        func unitsJSON() -> String {
            let items = units.map {
                #"{"id": \#($0.0), "name": "\#($0.1)", "bare": \#($0.2)}"#
            }
            return page(items)
        }

        /// Tags in the order that decides where this list's items sit.
        func tagOrderJSON(list: Int64) -> String {
            lock.lock()
            defer { lock.unlock() }

            let leading = placed[list] ?? []
            let ordered = leading.compactMap { id in tags.first { $0.0 == id } }
                + tags.filter { !leading.contains($0.0) }

            return "[\(ordered.map(tagJSON).joined(separator: ","))]"
        }

        func setTagOrder(_ ids: [Int64], on list: Int64) {
            lock.lock()
            defer { lock.unlock() }
            placed[list] = ids
        }

        private func tagJSON(_ tag: (Int64, String, Int64, String?)) -> String {
            """
            {"id": \(tag.0), "name": "\(tag.1)", "sort_order": \(tag.2),
             "emoji": \(tag.3.map { "\"\($0)\"" } ?? "null")}
            """
        }

        func tagsJSON() -> String {
            let items = tags.map { tag in
                """
                {"id": \(tag.0), "name": "\(tag.1)", "sort_order": \(tag.2),
                 "emoji": \(tag.3.map { "\"\($0)\"" } ?? "null")}
                """
            }
            return page(items)
        }

        func tagsOnItemJSON(_ id: Int64) -> String {
            lock.lock()
            defer { lock.unlock() }
            let held = rows.first { $0.id == id }?.tagIDs ?? []
            let items = tags.filter { held.contains($0.0) }.map { tag in
                """
                {"id": \(tag.0), "name": "\(tag.1)", "sort_order": \(tag.2),
                 "emoji": \(tag.3.map { "\"\($0)\"" } ?? "null")}
                """
            }
            return "[\(items.joined(separator: ","))]"
        }

        private func page(_ items: [String]) -> String {
            """
            {"items": [\(items.joined(separator: ","))], "total": \(items.count),
             "total_pages": 1, "has_more": false}
            """
        }

        // MARK: - Writing

        func setDone(_ id: Int64, _ done: Bool) {
            lock.lock()
            defer { lock.unlock() }
            guard let at = rows.firstIndex(where: { $0.id == id }) else { return }
            rows[at].doneAt = done ? "2026-08-26T10:00:00Z" : nil
        }

        func update(_ id: Int64, name: String, amount: Double, unitID: Int64?) {
            lock.lock()
            defer { lock.unlock() }
            guard let at = rows.firstIndex(where: { $0.id == id }) else { return }
            rows[at].name = name
            rows[at].amount = amount
            rows[at].unitID = unitID
        }

        func attach(_ tagID: Int64, to id: Int64) {
            lock.lock()
            defer { lock.unlock() }
            guard let at = rows.firstIndex(where: { $0.id == id }),
                  !rows[at].tagIDs.contains(tagID)
            else { return }
            // Kept in shop order, as the real route returns them.
            rows[at].tagIDs = (rows[at].tagIDs + [tagID]).sorted { lhs, rhs in
                (tags.first { $0.0 == lhs }?.2 ?? 0) < (tags.first { $0.0 == rhs }?.2 ?? 0)
            }
        }

        func detach(_ tagID: Int64, from id: Int64) {
            lock.lock()
            defer { lock.unlock() }
            guard let at = rows.firstIndex(where: { $0.id == id }) else { return }
            rows[at].tagIDs.removeAll { $0 == tagID }
        }

        func add(line: String, to list: Int64) {
            lock.lock()
            defer { lock.unlock() }
            // The real parsing happens on the server. This is the smallest thing that
            // behaves like it for the one shape the tests type.
            let next = (rows.map(\.id).max() ?? 0) + 1
            let parts = line.split(separator: " ").map(String.init)

            /// Adding what the list already wants changes nothing, as the service
            /// does: same name ignoring case, same unit. A crossed-off one comes back,
            /// with the amount it had.
            func put(_ name: String, _ amount: Double, _ unitID: Int64?) {
                let wanted = name.trimmingCharacters(in: .whitespaces).lowercased()
                let alike = rows.indices
                    .filter { rows[$0].listID == list }
                    .sorted { (rows[$0].doneAt != nil ? 1 : 0) < (rows[$1].doneAt != nil ? 1 : 0) }
                    .first {
                        rows[$0].unitID == unitID
                            && rows[$0].name.trimmingCharacters(in: .whitespaces)
                                .lowercased() == wanted
                    }

                if let at = alike {
                    // Not `+= amount`: it is already there, and somebody adding a thing
                    // has not looked at the number.
                    rows[at].doneAt = nil
                } else {
                    rows.append(
                        Row(
                            id: next, listID: list, name: name.capitalisedFirst,
                            amount: amount, unitID: unitID, doneAt: nil, tagIDs: []
                        )
                    )
                }
            }

            if parts.count >= 3, let amount = Double(parts[0]),
               let unit = units.first(where: { $0.1 == parts[1] })
            {
                put(parts.dropFirst(2).joined(separator: " "), amount, unit.0)
            } else {
                put(line, 1, nil)
            }
            itemsTotal = itemsTruncated ? itemsTotal : Int64(rows.count)
        }

        func delete(_ id: Int64) {
            lock.lock()
            defer { lock.unlock() }
            rows.removeAll { $0.id == id }
            itemsTotal = itemsTruncated ? itemsTotal : Int64(rows.count)
        }

        func clearDone() {
            lock.lock()
            defer { lock.unlock() }
            rows.removeAll { $0.doneAt != nil }
            itemsTotal = itemsTruncated ? itemsTotal : Int64(rows.count)
        }
    }

    extension String {
        /// The server capitalises names where it stores them; this keeps the fixture
        /// honest about what comes back.
        var capitalisedFirst: String {
            guard let first = first else { return self }
            return first.uppercased() + dropFirst()
        }
    }

#endif
