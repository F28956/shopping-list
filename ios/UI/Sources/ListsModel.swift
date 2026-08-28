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
    /// One backend, and this model no longer knows which. `CachingBackend` over a
    /// server, `LocalBackend` over the device's own database -- the cache and the queue
    /// live behind the first of those, because they exist to survive a *remote* that can
    /// fail rather than to serve one mode of the app.
    private let api: any Backend

    /// Who administers the server, when there is one.
    ///
    /// Absent on a device answering for itself, which is not a gap: there is no account
    /// to describe. What it decides here is one menu item, and the answer without a
    /// server is yes -- the person using the device administers it, which is the same
    /// answer `embedded` gives when it makes them an owner of their own database.
    private let accounts: (any Accounts)?



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
    init(api: any Backend, accounts: (any Accounts)? = nil) {
        self.api = api
        self.accounts = accounts

        // Nothing to watch here any more. The screen is kept in step by
        // `watchLists`, which consumes the backend's own stream -- SSE from a server,
        // `domain`'s broadcast channel from the device. One loop, either way.
    }
    // MARK: - Reading

    func load() async {
        do {
            let listing = try await api.lists()
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

            // What the backend is holding, and whether it got where it was going. Both
            // are read rather than inferred: a backend that answers from its memory
            // raises no error to infer from, which is exactly the trap the old code fell
            // into when it treated "no server" as "no signal".
            offline = !(await api.reachable)
            fresh = !offline
            waiting = await api.pending
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
            //
            // Rare now: `CachingBackend` answers reads from its memory rather than
            // failing, so this is what is left -- a write that could not be queued.
            offline = true
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

    /// Makes a list.
    ///
    /// No fallback here any more, and that is the change: a list made with no signal is
    /// still a list, but where it goes in the meantime is the backend's business.
    /// `CachingBackend` writes it down and queues it; `LocalBackend` has already stored
    /// it. This asks for a list and gets one.
    ///
    /// Answers it, so a caller that wants to select what it just made can.
    @discardableResult
    func makeList(named name: String) async -> List? {
        do {
            let made = try await api.createList(named: name)
            await load()
            return lists.first { $0.id == made.id } ?? made
        } catch {
            self.error = (error as? APIError)?.errorDescription ?? error.localizedDescription
            return nil
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
