import SwiftUI

/// The categories: what this app files things under.
///
/// In settings rather than on a list, because they belong to no one list — the same
/// twenty-one names order every list on a server, and renaming `dairy` renames it for
/// all of them. That is also why editing them is the owner's when there is a server:
/// it is the household's vocabulary, not one shopper's.
///
/// With no server there is no owner and no household, so anybody using the app may.
/// The two cases go to different places — the API, or this device's own cache — and
/// nothing else about the screen differs.
struct TagsView: View {
    let cache: Cache
    let api: API
    /// There is no server, so the edits are this device's own and go no further.
    let onDeviceOnly: Bool

    @Environment(\.dismiss) private var dismiss

    @State private var tags: [Tag] = []
    @State private var editing: Tag?
    @State private var adding = false
    @State private var deleting: Tag?
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                Section {
                    ForEach(tags) { tag in
                        Button {
                            editing = tag
                        } label: {
                            HStack(spacing: 12) {
                                // A fixed width so the names line up whether or not
                                // each has a glyph. Ragged, they read as two columns
                                // that disagree about where they start.
                                Text(tag.emoji ?? "")
                                    .frame(width: 24, alignment: .leading)
                                Text(tag.name)
                                Spacer()
                                Image(systemName: "chevron.right")
                                    .font(.footnote)
                                    .foregroundStyle(.tertiary)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("tag.\(tag.name)")
                        // Removing, for anywhere a swipe is not a gesture. `onDelete`
                        // below is a swipe and nothing else, so on a Mac there was no
                        // way to remove a category at all.
                        .contextMenu {
                            Button("Remove \(tag.name)", role: .destructive) {
                                deleting = tag
                            }
                        }
                    }
                    .onDelete { offsets in
                        deleting = offsets.first.map { tags[$0] }
                    }
                } footer: {
                    Text(
                        onDeviceOnly
                            ? "Items are grouped by these, in the order each list walks them. They stay on this device."
                            : "Items are grouped by these, in the order each list walks them. Everyone on this server shares them."
                    )
                }

                if let problem {
                    Section {
                        Text(problem)
                            .font(.footnote)
                            .foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("Categories")
            .compactTitle()
            // Adding on the left and finishing on the right, as Settings > Passwords
            // does — and along the bottom on a Mac, where a sheet's toolbar is not
            // drawn at all. See `sheetActions`.
            .sheetActions {
                Button("New category", systemImage: "plus") { adding = true }
                    .accessibilityIdentifier("tag.new")
            } finishing: {
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .task { load() }
            .sheet(item: $editing) { tag in
                TagEditor(tag: tag) { name, emoji in
                    await save(tag, name: name, emoji: emoji)
                }
            }
            .sheet(isPresented: $adding) {
                TagEditor(tag: nil) { name, emoji in
                    await add(name: name, emoji: emoji)
                }
            }
            .alert(item: $deleting) { tag in
                Alert(
                    title: Text("Remove \(tag.name)?"),
                    message: Text(
                        "Anything filed under it becomes unfiled. The items stay on their lists."
                    ),
                    primaryButton: .destructive(Text("Remove")) {
                        Task { await remove(tag) }
                    },
                    secondaryButton: .cancel()
                )
            }
        }
        .sheetSize()
    }

    private func load() {
        tags = cache.allTags()
    }

    private func save(_ tag: Tag, name: String, emoji: String?) async {
        // The cache first either way, so the screen answers immediately and a device
        // with no server has the whole answer. With one, the server is asked next and
        // has the final say — a refusal puts the old name back on the next load.
        cache.rename(tag: tag.id, to: name, emoji: emoji)
        load()

        guard !onDeviceOnly else { return }
        await attempt { _ = try await api.updateTag(tag, named: name, emoji: emoji) }
    }

    private func add(name: String, emoji: String?) async {
        guard !onDeviceOnly else {
            _ = cache.addTag(named: name, emoji: emoji)
            load()
            return
        }

        // Not written locally first: the id is the server's to mint, and a placeholder
        // would be a category that exists here under a number nothing else knows.
        await attempt {
            _ = try await api.createTag(named: name, emoji: emoji)
            try await refresh()
        }
    }

    private func remove(_ tag: Tag) async {
        cache.removeTag(tag.id)
        load()

        guard !onDeviceOnly else { return }
        await attempt { try await api.deleteTag(tag) }
    }

    /// Takes the server's categories as this device's, for every list.
    private func refresh() async throws {
        for list in cache.lists() {
            cache.remember(tags: try await api.tags(orderedFor: list), on: list)
        }
        load()
    }

    private func attempt(_ work: () async throws -> Void) async {
        do {
            try await work()
            problem = nil
        } catch let refusal as APIError {
            // A refusal here is nearly always "you do not own this server", which the
            // service hides as a 404. Said plainly rather than as "not found", which
            // would read as the category having vanished.
            problem = refusal.localizedDescription
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// One category's name and glyph.
private struct TagEditor: View {
    /// The category being changed, or nil when this is a new one.
    let tag: Tag?
    let save: (String, String?) async -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var name: String = ""
    @State private var emoji: String = ""

    var body: some View {
        NavigationStack {
            SwiftUI.List {
                Section {
                    TextField("Name", text: $name)
                        .accessibilityIdentifier("tag.name")
                    TextField("Glyph, optionally", text: $emoji)
                        .accessibilityIdentifier("tag.emoji")
                } footer: {
                    // One character, because the row shows it beside the name and a
                    // word there would push the name off a narrow screen.
                    Text("A single emoji, shown on every item filed here.")
                }
            }
            .navigationTitle(tag == nil ? "New category" : "Category")
            .compactTitle()
            .sheetActions {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
            } confirming: {
                Button("Save") {
                    let chosen = name.trimmingCharacters(in: .whitespacesAndNewlines)
                    let glyph = emoji.trimmingCharacters(in: .whitespacesAndNewlines)
                    dismiss()
                    Task { await save(chosen, glyph.isEmpty ? nil : glyph) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .onAppear {
                name = tag?.name ?? ""
                emoji = tag?.emoji ?? ""
            }
        }
        .sheetSize()
    }
}
