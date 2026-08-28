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

    private let api: API
    private let cache: Cache

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

    /// See ``ItemsModel/watching``: the model notices the cache moved, so that every
    /// screen built on it does, on both platforms.
    /// `nonisolated(unsafe)` because `deinit` is not on the main actor and this has to
    /// be read there to unsubscribe. Safe by construction: it is written once, in
    /// `init`, and read once, in `deinit` -- there is no moment when two things could
    /// touch it.
    nonisolated(unsafe) private var watching: (any NSObjectProtocol)?

    init(api: API, cache: Cache = .shared) {
        self.api = api
        self.cache = cache

        watching = NotificationCenter.default.addObserver(
            forName: .cacheChanged,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.reloadFromCache() }
        }
    }

    deinit {
        if let watching { NotificationCenter.default.removeObserver(watching) }
    }

    /// Re-reads the lists, because the cache says they changed.
    ///
    /// Unguarded by `fresh`, unlike `showWhatWeHave`: this is not a stale read racing a
    /// fresh one, it is the database saying it has moved since the last answer -- so it
    /// is the newer of the two by definition.
    func reloadFromCache() {
        let remembered = cache.lists()
        guard !remembered.isEmpty || lists.isEmpty else { return }
        lists = remembered
        waiting = cache.outbox.waiting
    }

    // MARK: - Reading

    /// Puts the last-loaded lists up before asking the server anything.
    ///
    /// The screen is never blank while a request is in flight, and on a device with no
    /// signal it is never blank at all. Guarded on `fresh` so a slow disk read cannot
    /// land after a fast answer and put yesterday's lists back.
    func showWhatWeHave() {
        guard !fresh else { return }
        let remembered = cache.lists()
        guard !remembered.isEmpty else { return }
        lists = remembered
        total = Int64(remembered.count)
        loaded = true
    }

    func load() async {
        do {
            let listing = try await api.lists()
            cache.remember(lists: listing.items)
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
            isOwner = (try? await api.whoAmI().isOwner) ?? isOwner

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
        } catch APIError.transport {
            let made = cache.makeListHere(named: name, ownedBy: mine)
            cache.outbox.makeList(made)
            waiting = cache.outbox.waiting
            lists = cache.lists()
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
        guard !draining, cache.outbox.waiting > 0 else { return }
        draining = true
        let drained = await cache.outbox.drain(through: api)
        draining = false
        waiting = cache.outbox.waiting

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
            lists = cache.lists()
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
