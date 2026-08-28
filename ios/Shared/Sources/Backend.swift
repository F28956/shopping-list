import Foundation

/// What answers the app's questions about shopping.
///
/// One protocol with one conformer today -- `API`, over HTTP -- and that is the whole
/// of this change. It is here to make a boundary visible before anything is moved
/// across it.
///
/// ## Why the surface is split in three
///
/// A device kept to itself and a device with a server are meant to be the same app,
/// and today they are the same app in the worst way: standalone is implemented as *a
/// server that fails every request* (`API.reachable`), so every screen goes down an
/// error path and is then told the error is not real. `onDeviceOnly` appears fifty-odd
/// times across eighteen files, and every one of those is a place that leaked out of.
///
/// The fix is not one protocol with everything in it. It is the observation that the
/// two modes differ in **what they offer**, not in **how shopping works**:
///
/// * ``Backend`` is shopping. A list, what is on it, what things are called and how
///   they are grouped. A device on its own can answer every one of these questions
///   from its own database -- which is what makes a local conformer possible, and what
///   would delete those fifty-odd branches.
/// * ``Accounts`` is who may sign in to a server. There is no answer to that without
///   one, and inventing one would be a lie.
/// * ``Sharing`` is who else is on a list. Likewise: a share link names a server, so
///   with none there is no link to make.
///
/// The second and third are **not** things a local conformer should implement badly.
/// They are things that should be *absent*, which is what the screens already do by
/// hiding them -- correctly, because offering to share when there is nobody to share
/// with is a worse app rather than a more uniform one.
///
/// See `docs/review.md` for the sequence this belongs to.
protocol Backend: Sendable {

    // MARK: - Reading

    func lists() async throws -> Listing<List>
    func items(on list: List) async throws -> Listing<Item>
    func units() async throws -> [Unit]
    func tags(orderedFor list: List) async throws -> [Tag]
    func tags(on item: Item, in list: List) async throws -> [Tag]
    func suggestions(matching typed: String, on list: List) async throws -> [String]
    func history(on list: List) async throws -> [RememberedEntry]

    // MARK: - Lists

    func createList(named name: String) async throws -> List
    func rename(_ list: List, to name: String) async throws
    func delete(_ list: List) async throws

    // MARK: - What is on one

    func add(_ line: String, to list: List) async throws
    func setDone(_ item: Item, on list: List, done: Bool) async throws
    /// By id, for a caller holding a queued operation rather than a row -- see the
    /// watch, which ticks things off it has only ever seen as numbers.
    func setDone(itemID: Int64, listID: Int64, done: Bool) async throws
    func update(
        _ item: Item,
        on list: List,
        name: String,
        amount: Double,
        unitID: Int64?
    ) async throws
    func attach(_ tag: Tag, to item: Item, on list: List) async throws
    func detach(_ tag: Tag, from item: Item, on list: List) async throws
    func clearDone(on list: List) async throws
    func delete(_ item: Item, on list: List) async throws

    // MARK: - The categories, which belong to no one list

    func setTagOrder(_ tags: [Tag], on list: List) async throws
    func createTag(named name: String, emoji: String?) async throws -> Tag
    func updateTag(_ tag: Tag, named name: String, emoji: String?) async throws -> Tag
    func deleteTag(_ tag: Tag) async throws

    // MARK: - Somebody else changed something

    /// The set of lists this person can see is not what they last read.
    ///
    /// A nudge and never the rows: a watcher told "something moved" and re-reading
    /// cannot drift, while one sent the new rows becomes a second opinion about them.
    func listChanges() async throws -> AsyncThrowingStream<Void, Error>

    /// This list is not what it was, and *what* about it changed.
    ///
    /// The kind matters because the answers cost different amounts. A tick means re-read
    /// the rows; a category renamed in Settings means re-read the vocabulary, which is
    /// thirty-one units and twenty-one categories. Told only that "something happened",
    /// a screen has to do both -- which is three requests per tick against a server, and
    /// is what this exists to stop.
    func changes(on list: List) async throws -> AsyncThrowingStream<Nudge, Error>

    // MARK: - What a screen can ask about the backend itself

    /// Whether the last attempt to reach the far end got there.
    ///
    /// The difference between "you have no lists" and "I could not find out", which is
    /// the bug the cache was built for. A screen reads this rather than inferring it
    /// from an error, because a backend that answers from its memory does not raise one.
    var reachable: Bool { get async }

    /// How much this backend is holding that has not reached where it is going.
    ///
    /// What the status dot counts.
    var pending: Int { get async }

    /// The rows on this list with work that has not been sent.
    ///
    /// Marked on the row rather than with a banner: it is a detail about that line, not
    /// news about the app.
    func unsent(on list: List) async -> Set<String>

    /// Sends whatever is waiting, and says what became of it.
    ///
    /// Called after a change and on a timer. Only the losses are worth showing:
    /// "three changes sent" is news about plumbing, while "the thing you crossed off had
    /// been deleted" is news about the list -- and the one case where somebody watched
    /// themselves do something that did not happen.
    @discardableResult
    func sync() async -> SyncReport
}

/// What moved.
enum Nudge: Sendable {
    /// What is on this list: something added, ticked off, corrected, filed or removed.
    case rows
    /// The categories themselves -- renamed, added, removed, or reordered for this
    /// list. Global, and changed from a screen that belongs to no list.
    case categories
}

/// What became of a queue.
struct SyncReport: Sendable {
    var sent = 0
    var waiting = 0
    /// Something was refused and will not retry itself. The one state worth
    /// interrupting somebody for.
    var refused = false
    /// Changes that can never land -- a tick against a row somebody else deleted.
    var lost: [String] = []
}

extension Backend {
    /// True, for a backend that is its own far end.
    ///
    /// `LocalBackend` cannot be out of reach of the device it is on, so the default is
    /// the answer rather than a placeholder. Only `CachingBackend` overrides it, because
    /// only it has somewhere to fail to get to.
    var reachable: Bool { get async { true } }

    /// Nothing, for a backend that has already stored what it was given.
    var pending: Int { get async { 0 } }

    /// Nothing unsent, for a backend that has already stored what it was given.
    func unsent(on list: List) async -> Set<String> { [] }

    /// Nothing to send. A device that is its own far end has already arrived.
    @discardableResult
    func sync() async -> SyncReport { SyncReport() }
}

/// Who may sign in to this server, and who this is.
///
/// Server-only, and deliberately not part of ``Backend``: a device with no server has
/// no account to describe and nobody to admit. A screen that needs this is a screen
/// that should be absent without one.
protocol Accounts: Sendable {
    func whoAmI() async throws -> Me
    func serverAbout() async throws -> ServerAbout
    func admissions() async throws -> [Admitted]
    func admit(_ email: String, note: String?) async throws
    func withdraw(_ email: String) async throws
    func setOwner(_ email: String, _ owner: Bool) async throws
    func setAdmitsAnyone(_ open: Bool) async throws
}

/// Who else is on a list.
///
/// Server-only for the same reason: a share link names a server, so with none there is
/// no link to make and nobody on the other end of one.
protocol Sharing: Sendable {
    func people(on list: List) async throws -> [Person]
    func invite(to list: List, as role: Role) async throws -> String
    func revokeInvites(to list: List) async throws
    func join(withToken token: String) async throws -> List
    func remove(_ person: Person, from list: List) async throws
}

extension Sharing {
    /// The role a share link gives unless somebody says otherwise.
    ///
    /// A protocol requirement cannot carry a default argument, so it is here instead --
    /// which keeps every existing `invite(to:)` call reading as it did.
    func invite(to list: List) async throws -> String {
        try await invite(to: list, as: .editor)
    }
}

// The one conformer, for now. `API` already has every one of these; nothing about it
// changed to satisfy this.
extension API: Backend {}
extension API: Accounts {}
extension API: Sharing {}
