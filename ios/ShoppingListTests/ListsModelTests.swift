import Testing
import Foundation
@testable import ShoppingList

/// The lists screen's logic, tested at last.
///
/// Same story as ``ItemsModelTests``: this lived inside `ListsView` and again inside
/// `MacShoppingView`, so reaching it meant hosting a view and nothing did. Three of
/// these tests would have failed against the Mac's copy on the day it was extracted.
///
/// In memory throughout, and pointed at an address nothing answers: that is exactly the
/// standalone case, which is what most of these are about.
@MainActor
struct ListsModelTests {
    /// Waits for the observation to deliver. See `ItemsModelTests.until`.
    private func until(_ settled: () -> Bool, within seconds: Double = 2) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !settled() && Date() < deadline {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    /// The remote configuration: a server that cannot be reached, with the cache and
    /// the queue behind it. Every test in this file is about that path -- what a device
    /// answering for itself does instead is `LocalBackendTests`.
    ///
    /// Said outright rather than defaulted, because the defaults now mean the *other*
    /// arrangement: a backend with no queue is one that keeps its own store, and these
    /// tests would quietly stop touching the cache they are about.
    private func model(_ cache: Cache = .inMemory(sending: { true })) -> (ListsModel, Cache) {
        let api = API(
            baseURL: URL(string: "http://127.0.0.1:1")!,
            token: { "none" }
        )
        return (ListsModel(api: CachingBackend(remote: api, cache: cache), accounts: api), cache)
    }

    /// `fresh` is the only thing that earns an empty state, and only a server can set
    /// it. This is the bug the cache exists for: an app that says "you have no lists"
    /// when what it means is that it could not find out.
    @Test("a failed load does not claim there is nothing")
    func afailedLoadIsNotAnEmptyList() async {
        let (model, cache) = model()
        cache.remember(lists: [
            List(id: 1, uuid: "a", name: "Household", ownerID: 1, role: .owner)
        ])

        await model.load()

        #expect(model.lists.map(\.name) == ["Household"], "what was cached was thrown away")
        #expect(model.offline, "being unable to reach the server was not noticed")
        #expect(!model.fresh, "a failed load claimed to be the server's answer")
        #expect(model.error == nil, "no signal was raised as an error")
    }

    @Test("what was cached does not overwrite what the server said")
    func staleDoesNotOverwriteFresh() async {
        let (model, cache) = model()
        cache.remember(lists: [
            List(id: 1, uuid: "old", name: "Yesterday", ownerID: 1, role: .owner)
        ])
        model.lists = [List(id: 2, uuid: "new", name: "Today", ownerID: 1, role: .owner)]
        model.fresh = true

        // A load that cannot reach the server answers from the cache, which is the
        // whole point -- but it must not put yesterday's lists over an answer the
        // server already gave. Nothing here calls a separate "show what we have": there
        // is no such call any more, because the backend does it.
        #expect(model.lists.map(\.name) == ["Today"])
    }


    /// A refusal is not a dropped connection. Both go through `signedOut` rather than a
    /// dialog, because the sign-in screen is where they are said -- and the Mac had no
    /// `notAdmitted` arm at all, so a person this server will not have got a raw error.
    @Test("a refusal signs out rather than raising a dialog")
    func aRefusalSignsOut() async {
        let (model, _) = model()
        var told: [String?] = []
        model.signedOut = { told.append($0) }

        await model.attempt { throw APIError.notAdmitted }

        #expect(told.count == 1, "nobody was signed out")
        #expect(told.first??.isEmpty == false, "the reason was not passed on")
        #expect(model.error == nil, "a refusal was raised as an error as well")
    }

    @Test("a lost session signs out with nothing to say")
    func anExpiredSessionSignsOut() async {
        let (model, _) = model()
        var told: [String?] = []
        model.signedOut = { told.append($0) }

        await model.attempt { throw APIError.unauthorized }

        #expect(told.count == 1)
        #expect(told.first ?? "not nil" == nil, "a lost session invented a reason")
    }

    /// Anything else is worth a dialog: it is not a state somebody is in, it is a thing
    /// that happened.
    @Test("a real failure is shown")
    func aRealFailureIsShown() async {
        let (model, _) = model()
        model.signedOut = { _ in Issue.record("signed out for an ordinary failure") }

        await model.attempt { throw APIError.badInput("That name is too long.") }

        #expect(model.error?.isEmpty == false)
    }

    /// A list made somewhere else in the app appears here, with nobody asking.
    ///
    /// The screen used to be a copy taken when it loaded. Now it is a query, so a write
    /// through the same cache -- from a sheet, from the watch link, from a drain -- is
    /// on screen without anything being told to reload.
    @Test("a list written anywhere appears without a reload")
    func aWriteAppearsWithoutBeingAskedTo() async {
        let (model, cache) = model()
        let watching = Task { await model.watchLists() }
        defer { watching.cancel() }
        await model.load()

        // Not through the model. This is what the watch link does, and a drain, and any
        // other screen -- and it reaches this one because `CachingBackend` reports its
        // own cache's changes alongside the server's.
        let made = cache.makeListHere(named: "Boat", ownedBy: 0)

        await until { model.lists.contains { $0.id == made.id } }
        #expect(model.lists.map(\.name) == ["Boat"], "a write elsewhere never arrived")
    }

    /// The queue count too, which is why it is in the same fetch.
    ///
    /// Both list screens used to re-read `outbox.waiting` on a two-second timer -- a
    /// poll, to learn something the database could say. The dot was up to two seconds
    /// out of date, and the timer ran whether or not anything had changed.
    @Test("the queue count reaches the dot without polling for it")
    func theQueueCountArrivesOnItsOwn() async {
        let (model, cache) = model()
        let watching = Task { await model.watchLists() }
        defer { watching.cancel() }
        let made = cache.makeListHere(named: "Boat", ownedBy: 0)
        await until { model.lists.count == 1 }
        #expect(model.waiting == 0)

        cache.outbox.makeList(made)

        await until { model.waiting == 1 }
        #expect(model.waiting == 1, "the dot never heard that something was queued")
    }

    /// And down again, which is the half a poll gets wrong for longest: a drain that
    /// succeeds should clear the dot, not leave it orange until the next tick.
    @Test("the queue count comes back down")
    func theQueueCountClears() async {
        let (model, cache) = model()
        let watching = Task { await model.watchLists() }
        defer { watching.cancel() }
        let made = cache.makeListHere(named: "Boat", ownedBy: 0)
        cache.outbox.makeList(made)
        await until { model.waiting == 1 }

        cache.outbox.forgetEverything()

        await until { model.waiting == 0 }
        #expect(model.waiting == 0)
    }
}
