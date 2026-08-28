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
        return (ListsModel(api: api, accounts: api, queue: api, cache: cache), cache)
    }

    /// The one the Mac got wrong. It called `api.createList` and showed the failure,
    /// so on a machine deliberately kept off a server the only button on the screen
    /// raised a dialog and made nothing.
    ///
    /// Standalone: written down, and **nothing queued**, because there is nobody to
    /// tell. Adopting a server later is what `handOverIfNeeded` is for.
    @Test("a list can be made with no server at all")
    func makingWorksWithNoServer() async {
        let (model, cache) = model(.inMemory(sending: { false }))

        let made = await model.makeList(named: "Household")

        #expect(made?.name == "Household")
        #expect(model.lists.map(\.name) == ["Household"], "the list did not reach the screen")
        #expect(model.error == nil, "making a list offline was reported as a failure")
        #expect(cache.lists().map(\.name) == ["Household"], "it was not written down")
        #expect(cache.outbox.waiting == 0, "a device with nobody to tell queued something")
        #expect(model.waiting == 0, "the dot claimed something was waiting to be sent")
    }

    /// The other mode, which is a different thing and looks the same from the screen:
    /// there **is** a server, it just cannot be reached. Here the change is queued,
    /// because there is somebody who has not been told yet.
    @Test("a list made while the server is unreachable is queued for it")
    func makingQueuesWhenAServerExists() async {
        let (model, cache) = model(.inMemory(sending: { true }))

        let made = await model.makeList(named: "Household")

        #expect(made?.name == "Household")
        #expect(cache.lists().map(\.name) == ["Household"])
        #expect(cache.outbox.waiting == 1, "nothing was queued for a server that exists")
        #expect(model.waiting == 1, "the screen was not told there is something waiting")
    }

    @Test("a list made with no server is still there after a restart")
    func makingSurvivesTheProcess() async {
        let path = NSTemporaryDirectory() + "lists-\(UUID().uuidString).sqlite"
        defer { try? FileManager.default.removeItem(atPath: path) }

        let (first, _) = model(Cache(path: path, sending: { false }))
        await first.makeList(named: "Household")

        // A second cache over the same file is the only honest way to test this: the
        // queue outliving the process is the whole point of it.
        let (second, _) = model(Cache(path: path, sending: { false }))
        second.showWhatWeHave()

        #expect(second.lists.map(\.name) == ["Household"])
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

        model.showWhatWeHave()
        await model.load()

        #expect(model.lists.map(\.name) == ["Household"], "what was cached was thrown away")
        #expect(model.offline, "being unable to reach the server was not noticed")
        #expect(!model.fresh, "a failed load claimed to be the server's answer")
        #expect(model.error == nil, "no signal was raised as an error")
    }

    /// Guarded on `fresh` so a slow disk read cannot land after a fast answer and put
    /// yesterday's lists back.
    @Test("what was cached does not overwrite what the server said")
    func staleDoesNotOverwriteFresh() async {
        let (model, cache) = model()
        cache.remember(lists: [
            List(id: 1, uuid: "old", name: "Yesterday", ownerID: 1, role: .owner)
        ])
        model.lists = [List(id: 2, uuid: "new", name: "Today", ownerID: 1, role: .owner)]
        model.fresh = true

        model.showWhatWeHave()

        #expect(model.lists.map(\.name) == ["Today"])
    }

    /// The half of the drain that is reachable with no server. What the Mac got wrong
    /// is the other half -- it never swapped this device's numbering for the server's,
    /// so a list made offline would have appeared twice, once under each id -- and
    /// that needs a server to answer before it can be asserted on. Named here so the
    /// gap is a known one rather than an assumed pass.
    @Test("a drain with nothing to send changes nothing")
    func drainingAnEmptyQueueIsQuiet() async {
        let (model, cache) = model(.inMemory(sending: { true }))
        await model.makeList(named: "Household")
        let before = model.lists

        await model.sendQueued()

        #expect(model.lists == before, "an undeliverable drain disturbed the screen")
        #expect(cache.outbox.waiting == 1, "the queue was emptied without a server")
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
        await until { model.lists.isEmpty }

        // Not through the model. This is what another part of the app does.
        let made = cache.makeListHere(named: "Boat", ownedBy: 0)

        await until { model.lists.contains { $0.id == made.id } }
        #expect(model.lists.map(\.name) == ["Boat"])
    }

    /// The queue count too, which is why it is in the same fetch.
    ///
    /// Both list screens used to re-read `outbox.waiting` on a two-second timer -- a
    /// poll, to learn something the database could say. The dot was up to two seconds
    /// out of date, and the timer ran whether or not anything had changed.
    @Test("the queue count reaches the dot without polling for it")
    func theQueueCountArrivesOnItsOwn() async {
        let (model, cache) = model()
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
        let made = cache.makeListHere(named: "Boat", ownedBy: 0)
        cache.outbox.makeList(made)
        await until { model.waiting == 1 }

        cache.outbox.forgetEverything()

        await until { model.waiting == 0 }
        #expect(model.waiting == 0)
    }
}
