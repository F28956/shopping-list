import Foundation
import Observation

/// The lists this person can see, and everything that can be done to them.
///
/// The smaller sibling of ``ItemsModel``, extracted for the same reason and after the
/// same evidence. `ListsView` and `MacShoppingView` held their own copies of this --
/// `watchLists`, `attempt` and `showWhatWeHave` byte-identical, `load` and `sendQueued`
/// nearly so -- and the copies had already drifted in three places, all of them on the
/// Mac, all of them in the direction of the Mac being worse:
///
/// * **A list could not be made with no server.** The Mac called `api.createList` and
///   showed the failure in a dialog. On a machine somebody deliberately set to work on
///   its own, the one button on the screen did nothing but complain. The phone had
///   queued it locally since S1.
/// * **Lists made offline appeared twice.** The phone swaps this device's numbering for
///   the server's after a drain; the Mac never did, so a list would have shown once
///   under each id.
/// * **A person the server will not have got a raw error dialog** instead of being told
///   on the sign-in screen, because the Mac's `load` handled `unauthorized` and
///   `transport` but not `notAdmitted`.
///
/// None of those were decisions. They are what happens when the same logic is written
/// twice and only one copy is maintained.
@MainActor
@Observable
final class ListsModel {

    /// Shopping, plus the one question about the account: whether this person
    /// administers the server, which decides whether a screen exists. `Accounts` is
    /// separate from ``Backend`` because a device with no server has no answer to it --
    /// see the note there.
    /// `Destination` as well, because this screen is where the queue is emptied --
    /// see `sendQueued`. It is a separate protocol from ``Backend`` deliberately: the
    /// queue's other conformer is the watch's link to its phone, which is not a server
    /// and does not pretend to be one.
    private let api: any Backend

    /// Who administers the server, when there is one.
    ///
    /// Absent on a device answering for itself, which is not a gap: there is no account
    /// to describe. What it decides here is one menu item, and the answer without a
    /// server is yes -- the person using the device administers it, which is the same
    /// answer `embedded` gives when it makes them an owner of their own database.
    private let accounts: (any Accounts)?

    /// Where the queue goes, when there is a queue.
    ///
    /// Absent for a backend that keeps its own store, and that absence is the whole
    /// transition: a queue exists because a remote can fail, and one that cannot has
    /// nothing to queue. `keepsItsOwnStore` reads off it rather than off a flag, so the
    /// two cannot disagree.
    private let queue: (any Destination)?

    private let cache: Cache

    /// Whether the backend is its own memory.
    ///
    /// True for `LocalBackend`, where the device's database *is* the answer, so the
    /// cache is neither read nor written and updates arrive from the backend's own
    /// stream. False for `API`, which is remote, may fail, and needs both.
    ///
    /// A fork, and a deliberately temporary one. The end of this is a `CachingBackend`
    /// wrapping `API` so that the cache and the queue live behind the protocol and this
    /// model asks no such question -- but that shape should be argued for by a screen
    /// that has actually run both ways, which is what this is.
    private var keepsItsOwnStore: Bool { queue == nil }

    /// Says this person is no longer signed in, and why if there is a reason.
    ///
    /// A closure rather than a reach for `Identity`, for the reason given on
    /// ``ItemsModel/signedOut``: signing out tears down the screen this object is
    /// attached to, so it belongs to the app rather than to the model.
    var signedOut: ((String?) -> Void)?

    // MARK: - What is on screen

    var lists: [List] = []
    var total: Int64 = 0
    var truncated = false

    /// Whether anything has been shown yet. The spinner is on until it has.
    var loaded = false
    /// Whether the server has ever answered. What is shown while this is false came out
    /// of the cache and may be old -- and a failed load with nothing cached is not
    /// evidence that somebody has no lists, which was the bug ``Cache`` exists for.
    var fresh = false
    /// The server could not be reached last time we asked. Not an error and not worth a
    /// dialog -- but the difference between "you have no lists" and "I could not find
    /// out" has to reach the screen.
    var offline = false
    /// How many changes are waiting, anywhere. This screen is where the app opens, so
    /// it is where somebody first sees whether this device is in step.
    var waiting = 0
    /// Whether this person administers this server, which decides whether the screen
    /// that manages it exists. Hiding it is a courtesy: every route behind it is
    /// refused in the service layer to anybody else.
    var isOwner = false
    var error: String?

    /// Guards against a drain and a reload calling each other round in a circle.
    private var draining = false
    /// See ``ItemsModel/watching``, which explains the whole of this: the screen is a
    /// query over the database rather than a copy of what it once said.
    ///
    /// `nonisolated(unsafe)` because `deinit` is not on the main actor and has to cancel
    /// it. Safe by construction: written once in `init`, read once in `deinit`.
    nonisolated(unsafe) private var watching: Task<Void, Never>?

    init(
        api: any Backend,
        accounts: (any Accounts)? = nil,
        queue: (any Destination)? = nil,
        cache: Cache = .shared
    ) {
        self.api = api
        self.accounts = accounts
        self.queue = queue
        self.cache = cache

        watching = Task { [weak self] in
            // A backend that keeps its own store tells the screen itself -- see
            // `watchLists`, which is where that stream is consumed. There is nothing in
            // the cache to observe, because nothing writes to it.
            guard queue != nil else { return }

            while !Task.isCancelled {
                guard let stream = cache.observeLists() else { return }
                do {
                    for try await overview in stream {
                        guard let self else { return }
                        self.adopt(overview)
                    }
                    return
                } catch {
                    // Restarted rather than given up on: a screen that has quietly
                    // stopped updating looks exactly like one that is working.
                    try? await Task.sleep(for: .seconds(1))
                }
            }
        }
    }

    deinit {
        watching?.cancel()
    }

    /// What the database currently says, put on screen.
    ///
    /// The guard is about emptiness rather than staleness: a cache not filled yet must
    /// not blank a screen the server has already answered. `waiting` has no such guard
    /// -- nothing queued is a real answer, and the honest one the moment a drain lands.
    private func adopt(_ overview: Cache.Overview) {
        if !overview.lists.isEmpty || lists.isEmpty {
            lists = overview.lists
            if !fresh { total = Int64(overview.lists.count) }
        }
        waiting = overview.waiting
        if !lists.isEmpty { loaded = true }
    }

    /// Re-reads the lists and the queue.
    ///
    /// Kept for the tests, and for the few places that want the answer in this turn
    /// rather than on the observation's next one. Nothing else calls it.
    func reloadFromCache() {
        // Nothing to re-read: the cache is not this backend's memory, and reading it
        // would put another backend's lists on screen. This is the sharpest edge of
        // running two stores side by side, and the reason the old one is read-only for
        // the duration rather than merely unused.
        guard !keepsItsOwnStore else { return }
        guard let overview = cache.overview() else { return }
        adopt(overview)
    }

    // MARK: - Reading

    /// Puts the last-loaded lists up before asking the server anything.
    ///
    /// The screen is never blank while a request is in flight, and on a device with no
    /// signal it is never blank at all. Guarded on `fresh` so a slow disk read cannot
    /// land after a fast answer and put yesterday's lists back.
    func showWhatWeHave() {
        guard !fresh else { return }
        reloadFromCache()
    }

    func load() async {
        do {
            let listing = try await api.lists()
            // Not written down when the backend is already the place it would be
            // written to. The cache is left exactly as the old path left it, so that
            // path still works if this one has to be backed out.
            if !keepsItsOwnStore { cache.remember(lists: listing.items) }
            lists = listing.items
            total = listing.total
            truncated = listing.truncated
            error = nil
            offline = false
            fresh = true

            // Asked once the lists have arrived rather than beside them, because
            // nothing on this screen waits for it -- a menu item appearing a moment
            // late is better than a screen that waits for a question about
            // administration before it shows anybody their shopping.
            // Yes without a server: the person using the device administers it, which
            // is what `embedded` says too when it makes them an owner of their own
            // database. Asked otherwise, and asked after the lists have arrived, because
            // nothing on this screen waits for it.
            if let accounts {
                isOwner = (try? await accounts.whoAmI().isOwner) ?? isOwner
            } else {
                isOwner = true
            }

            // The server is reachable, so anything queued anywhere goes now.
            //
            // Here as well as on the list screen, because the app opens here: a phone
            // that came out of a shop and was put in a pocket would otherwise hold its
            // ticks until somebody happened to open the list they were made on.
            await sendQueued()
        } catch let problem as APIError {
            report(problem)
        } catch {
            self.error = error.localizedDescription
        }
        loaded = true
    }

    /// What a failed load means, in one place.
    ///
    /// Three of the four cases are deliberately not errors on screen, and each for its
    /// own reason. The Mac was missing the `notAdmitted` arm entirely, which is how a
    /// person this server will not have ended up looking at a dialog instead of the
    /// sentence written for them.
    private func report(_ problem: APIError) {
        switch problem {
        case .unauthorized:
            // Not an error worth a dialog: the root view puts the sign-in screen back
            // as soon as the state changes.
            signedOut?(nil)
        case .notAdmitted:
            // Not a person with an empty list -- a person this server will not have.
            // The sign-in screen is where that is said, and it is said once rather than
            // raised again every time the stream reconnects.
            signedOut?(problem.localizedDescription)
        case .transport:
            // Not shown. Being out of signal is a state, not an event: a phone in a
            // basement would raise this every few seconds, and a dialog for each is an
            // interruption on top of an app that is still usable.
            offline = true
            if !fresh { showWhatWeHave() }
        default:
            error = problem.localizedDescription
        }
    }

    // MARK: - Changing

    /// Runs something that changes the lists, then reloads.
    func attempt(_ work: () async throws -> Void) async {
        do {
            try await work()
            await load()
        } catch let problem as APIError {
            if case .unauthorized = problem {
                signedOut?(nil)
            } else if case .notAdmitted = problem {
                signedOut?(problem.localizedDescription)
            } else {
                error = problem.localizedDescription
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Makes a list, wherever it can.
    ///
    /// The server first, because a list made online should arrive with an id and no
    /// queue behind it. A transport failure is not an error here and never shows one:
    /// no signal and no server are the same state, and writing the list down locally is
    /// what the person asked for either way. It is queued, and the queue is what carries
    /// it to a server if one ever appears.
    ///
    /// This is S1 -- the app is useful before it has anywhere to send anything. The Mac
    /// did not have it, so on a Mac deliberately kept off a server the only button on
    /// the screen raised a dialog.
    ///
    /// Answers the list, so a caller that wants to select what was just made can.
    @discardableResult
    func makeList(named name: String) async -> List? {
        do {
            let made = try await api.createList(named: name)
            await load()
            return lists.first { $0.id == made.id } ?? made
        } catch APIError.transport where !keepsItsOwnStore {
            let made = cache.makeListHere(named: name, ownedBy: mine)
            cache.outbox.makeList(made)
            // The observation will deliver this too, a hop later. Read now as well so
            // the row is there in the same turn -- a caller that selects what it just
            // made would otherwise select something not yet on screen. Through the same
            // fetch, not field by field, so the two cannot say different things.
            reloadFromCache()
            offline = true
            return made
        } catch {
            self.error = (error as? APIError)?.errorDescription ?? error.localizedDescription
            return nil
        }
    }

    /// This person's id, for a list made with nobody to ask.
    ///
    /// Zero where there is no server and so no account. It is only ever compared with
    /// itself on this device -- the server decides ownership from who sent the
    /// operation, not from what the device claimed.
    private var mine: Int64 { 0 }

    /// Empties the outbox, wherever its contents belong.
    ///
    /// A change queued on any list goes: the operation carries the list it was made
    /// against, so nothing here needs to know which screen it came from. Failures are
    /// the outbox's business -- see ``Outbox/drain(through:)`` -- and what is left stays
    /// queued for the next successful load.
    func sendQueued() async {
        // Nothing to send, and nowhere to send it. A backend that keeps its own store
        // has already stored it.
        guard let queue else { return }

        // Before the count is looked at, because it is what puts something in the queue
        // on a device that has just been given a server: nothing was queued while it had
        // none, so the lists made there are known to this device and to nothing else.
        cache.handOverIfNeeded()

        guard !draining, cache.outbox.waiting > 0 else { return }
        draining = true
        let drained = await cache.outbox.drain(through: queue)
        draining = false
        reloadFromCache()

        // Lists made here have just been given the server's own ids. Done before the
        // reload below, so the screen never shows the same list twice -- once under this
        // device's numbering and once under the server's. The Mac never did this, so on
        // the Mac it would have.
        for adopted in drained.adopted {
            if let local = cache.lists().first(where: { $0.uuid == adopted.uuid }) {
                cache.adopt(local, as: adopted.real)
            }
        }

        if !drained.adopted.isEmpty {
            reloadFromCache()
        }
    }

    // MARK: - Staying in step

    /// Keeps the screen in step with lists made, renamed, deleted or joined anywhere.
    ///
    /// A list's own stream cannot carry this: one that has just been made has no
    /// watchers at all, which is why a list created on a phone never appeared here.
    func watchLists() async {
        var reconnecting = false

        while !Task.isCancelled {
            if reconnecting { await load() }

            do {
                for try await _ in try await api.listChanges() {
                    await load()
                }
            } catch let problem as APIError {
                if case .unauthorized = problem {
                    signedOut?(nil)
                    return
                }
                // A refusal is not a dropped connection. Reconnecting every three
                // seconds to be refused again is a loop that ends only when somebody
                // closes the app, and each turn of it used to raise another dialog.
                if case .forbidden = problem { return }
                if case .notAdmitted = problem {
                    signedOut?(problem.localizedDescription)
                    return
                }
            } catch {
                // Anything else is the connection going away -- a tunnel, a lock
                // screen, a server restarting. Ordinary, and not worth showing; the
                // wait below and the loop are the whole response.
            }

            // Waiting keeps a server that is refusing everything from being asked as
            // fast as the loop can go round.
            reconnecting = true
            try? await Task.sleep(for: .seconds(3))
        }
    }
}
