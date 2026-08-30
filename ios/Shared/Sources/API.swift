import Foundation

/// What went wrong, in terms the screen can answer.
enum APIError: LocalizedError {
    /// The server did not accept the token. Usually means signed out, or that the
    /// server has not been told about this app's client id.
    case unauthorized
    case notFound
    /// The service refused: a viewer trying to change something.
    case forbidden
    /// This account may not use this server at all.
    ///
    /// Shares 403 with `forbidden` and is a different thing entirely: that one is a
    /// sentence about a list, this one is a sentence about the account, and asking
    /// again will not change it. Told apart by the `reason` in the body, because the
    /// status cannot tell them apart -- and when nothing did, somebody signing in
    /// with an unlisted address was told they could read a list they did not have.
    case notAdmitted
    case badInput(String)
    case server(Int)
    case transport(Error)

    var errorDescription: String? {
        switch self {
        case .unauthorized:
            return "Signed out. Sign in again."
        case .notFound:
            return "That is not there any more."
        case .forbidden:
            return "You can look at this list but not change it."
        case .notAdmitted:
            return "This account is not allowed to use this server."
        case .badInput(let what):
            return what
        case .server(let code):
            return "The server had a problem (\(code))."
        case .transport(let error):
            return error.localizedDescription
        }
    }
}

/// The API, as this app uses it.
///
/// Every call carries a bearer token and nothing else: the API never reads cookies,
/// which is what lets it share an origin with the web UI safely.
actor API {
    /// Asked per request rather than held, because on the watch the answer arrives
    /// after the app has started: the address comes from the paired phone alongside
    /// the token (C5), and an `API` built at launch would otherwise be pointed at
    /// nothing for the whole first run.
    private let server: @Sendable () -> URL

    private var baseURL: URL { server() }

    /// Whether there is anywhere to send anything.
    ///
    /// False on a device somebody chose to keep to itself. Every call then fails as a
    /// transport error before a socket is opened -- which is not a workaround but the
    /// design: "no server" and "no signal" are the same state, and the app has known
    /// how to be in one of them since the offline work. The cache answers, the outbox
    /// fills, and attaching a server later drains it.
    ///
    /// **Asked of this instance rather than of a global**, and that distinction was a
    /// bug. It read `ServerDirectory.current`, which is *this device's* stored choice --
    /// right for the phone and the Mac, and wrong for the watch, which is handed an
    /// address by its phone and does not store one of its own. A watch told about a
    /// server therefore built an `API` pointed straight at it and then refused every
    /// request with "This device is not using a server", holding a perfectly good
    /// address the whole time. On a list whose rows were cached the error was invisible;
    /// on an empty one it was the entire screen.
    private let hasSomewhereToSend: @Sendable () -> Bool

    private var reachable: Bool { hasSomewhereToSend() }
    private let session: URLSession
    private let token: () async -> String?
    /// Whether somebody is signed in on this device, whether or not there is a token to
    /// hand right now.
    ///
    /// The two are different questions offline. Google cannot refresh a token without a
    /// connection, and treating that as "signed out" would put the sign-in screen in
    /// front of somebody whose own list is sitting on the phone — so a missing token
    /// with a remembered session is reported as a transport failure, which is what it
    /// is.
    private let remembered: () -> Bool

    /// A fixed address. What the phone and the Mac use, where the answer is known
    /// before anything is built.
    init(
        baseURL: URL,
        token: @escaping () async -> String?,
        remembered: @escaping () -> Bool = { false }
    ) {
        // This device's own choice, which is what a fixed address means: the phone and
        // the Mac are pointed at `Config.apiBaseURL` whether or not anybody has asked
        // for a server, so something has to say which.
        self.init(
            server: { baseURL },
            token: token,
            remembered: remembered,
            hasSomewhereToSend: { ServerDirectory.current != nil }
        )
    }

    /// An address that may not be known yet -- the watch, which learns it from its
    /// phone.
    ///
    /// `hasSomewhereToSend` defaults to true here, and that is the point of the
    /// separate initialiser: a caller that supplies an address has one. A watch with no
    /// server does not build one of these at all -- see `WatchStore.destination`, which
    /// answers the phone instead.
    init(
        server: @escaping @Sendable () -> URL,
        token: @escaping () async -> String?,
        remembered: @escaping () -> Bool = { false },
        hasSomewhereToSend: @escaping @Sendable () -> Bool = { true }
    ) {
        self.server = server
        self.token = token
        self.remembered = remembered
        self.hasSomewhereToSend = hasSomewhereToSend

        let configuration = URLSessionConfiguration.default
        #if DEBUG
            // Under UI test the wire is replaced and nothing above it is: the same
            // URLs, headers, status handling and decoding all still run.
            if UITesting.isRunning {
                configuration.protocolClasses = [StubURLProtocol.self]
            }
        #endif
        self.session = URLSession(configuration: configuration)
    }

    // MARK: - Reading

    /// The service's own ceiling, and the same one the browser asks for.
    ///
    /// Asking for less is how these screens came to show a prefix of a list without
    /// saying so: the default page is twenty, and twenty-one lists meant the last one
    /// simply was not there. Anything past this is reported rather than dropped.
    static let pageLimit = 500

    func lists() async throws -> Listing<List> {
        let page: Page<List> = try await get(
            "/api/lists?order_by=updated_at&direction=descending&size=\(Self.pageLimit)"
        )
        return Listing(page)
    }

    func items(on list: List) async throws -> Listing<Item> {
        // Outstanding first, then what is already in the trolley — the same order the
        // web UI uses, so the two do not show the same list differently.
        let page: Page<Item> = try await get(
            "/api/lists/\(list.id)/items"
                + "?order_by=done_at&direction=ascending&size=\(Self.pageLimit)"
        )
        return Listing(page)
    }

    /// Every unit, in name order.
    ///
    /// An array rather than a lookup because the editor's picker needs an order and a
    /// dictionary has none. Rows still want the lookup, and the screen builds it.
    func units() async throws -> [Unit] {
        // Reference data, seeded by migration and counted in dozens. The ceiling is
        // an order of magnitude clear of it, so truncation here would mean a
        // deployment gone wrong rather than a list somebody grew.
        let page: Page<Unit> = try await get("/api/units?order_by=name&size=\(Self.pageLimit)")
        return page.items
    }

    /// Every tag, in the order that decides where this list's items sit.
    ///
    /// Not `/api/tags`, which is one global opinion. This is resolved per person and
    /// per list by the service, so grouping reads position in this answer and needs
    /// no second opinion about what leads.
    func tags(orderedFor list: List) async throws -> [Tag] {
        try await get("/api/lists/\(list.id)/tag-order")
    }

    /// Puts these tags in front for this person on this list. An empty array clears
    /// the choice, putting them back on whatever they inherit.
    func setTagOrder(_ tags: [Tag], on list: List) async throws {
        _ = try await send(
            "PUT",
            "/api/lists/\(list.id)/tag-order",
            ["tag_ids": tags.map(\.id)]
        )
    }

    /// What this item is filed under. A bare array, not a page: an item has few.
    func tags(on item: Item, in list: List) async throws -> [Tag] {
        try await get("/api/lists/\(list.id)/items/\(item.id)/tags")
    }

    /// What gets bought on this list that matches what has been typed, best first.
    ///
    /// Matched on the server, not here. The rules are loose -- letters need not be
    /// adjacent or at the start -- and a second implementation of them in Swift would
    /// agree with the browser only until one of the two was changed. The order is the
    /// server's too, so this shows what it is given and does not re-sort.
    /// Capped and de-duplicated by the service, so this asks and shows what it is
    /// given: the browser and the phone offered different numbers of different things
    /// for the same letters when each decided for itself.
    func suggestions(matching typed: String, on list: List) async throws -> [String] {
        let query = typed.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? ""
        return try await get("/api/lists/\(list.id)/history?q=\(query)")
    }

    /// The whole of what this list remembers.
    ///
    /// Not the same question as `suggestions`, which asks what to offer for some
    /// letters and answers with names. This is what the device needs to resolve a
    /// typed line the way the server would: the unit, how much, and where it is filed.
    /// Kept, so the answer is the same with or without a connection — and so the
    /// memory is the household's rather than each device's.
    func history(on list: List) async throws -> [RememberedEntry] {
        try await get("/api/lists/\(list.id)/history/entries")
    }

    // MARK: - Writing

    // MARK: - The aisles

    /// Makes an aisle. Refused unless this person owns the server.
    ///
    /// A tag is not one person's — it is the vocabulary every list on the server is
    /// filed under — which is why it is the owner's to change and nobody else's.
    func createTag(named name: String, emoji: String?) async throws -> Tag {
        try Self.decoder.decode(
            Tag.self,
            from: try await send("POST", "/api/tags", body(name, emoji))
        )
    }

    /// Renames one, or changes its glyph.
    ///
    /// A whole replacement rather than a patch: leaving `emoji` out is how somebody
    /// removes one, and a partial update has no way to say that.
    func updateTag(_ tag: Tag, named name: String, emoji: String?) async throws -> Tag {
        try Self.decoder.decode(
            Tag.self,
            from: try await send("PATCH", "/api/tags/\(tag.id)", body(name, emoji))
        )
    }

    /// Removes one. What was filed under it becomes unfiled, and it leaves everyone's
    /// walking order — the server cascades both.
    func deleteTag(_ tag: Tag) async throws {
        _ = try await send("DELETE", "/api/tags/\(tag.id)", nil)
    }

    /// An emoji that is absent and one that is empty mean the same thing — no glyph —
    /// and the model turns one into the other, so either spelling is fine to send.
    private func body(_ name: String, _ emoji: String?) -> [String: Any] {
        var fields: [String: Any] = ["name": name]
        if let emoji, !emoji.isEmpty { fields["emoji"] = emoji }
        return fields
    }

    // MARK: - Sharing

    /// Who this is, so a screen can tell which member is you.
    func whoAmI() async throws -> Me {
        try await get("/api/me")
    }

    // MARK: - Administering the server

    /// What this server says about itself, including whether it admits anybody.
    func serverAbout() async throws -> ServerAbout {
        try await get("/api/server")
    }

    /// Every address that may sign in. Owners only — anybody else gets a refusal.
    func admissions() async throws -> [Admitted] {
        try await get("/api/admissions")
    }

    /// Lets an address sign in. Admitting one twice is a double-click, not an error.
    func admit(_ email: String, note: String?) async throws {
        var body: [String: Any] = ["email": email]
        if let note, !note.isEmpty { body["note"] = note }
        try await send("POST", "/api/admissions", body)
    }

    /// Takes an address off the list. Takes effect on that person's very next request,
    /// not whenever their session happens to expire.
    func withdraw(_ email: String) async throws {
        try await send("DELETE", "/api/admissions/\(escaped(email))", nil)
    }

    /// Makes somebody an owner, or stops them being one.
    ///
    /// The server refuses the last owner being demoted, and refuses promoting somebody
    /// who has never signed in — there is no person yet to make an owner.
    func setOwner(_ email: String, _ owner: Bool) async throws {
        let path = "/api/admissions/\(escaped(email))/owner"
        try await send(owner ? "POST" : "DELETE", path, nil)
    }

    /// Opens the server to anybody a provider vouches for, or closes it again.
    func setAdmitsAnyone(_ open: Bool) async throws {
        try await send("PUT", "/api/server", ["admits_anyone": open])
    }

    /// An address is a path component here, and addresses contain `+` and `@`.
    private func escaped(_ email: String) -> String {
        email.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? email
    }

    // MARK: - Sharing continued

    /// Everyone who can see this list, the owner first.
    func people(on list: List) async throws -> [Person] {
        try await get("/api/lists/\(list.id)/members")
    }

    /// A code to share, returned once and never again — only its hash is stored, so a
    /// caller that loses it makes another rather than looking the old one up.
    ///
    /// The code alone, not a link. A link carries a host, and the host these apps talk
    /// to is a laptop on somebody's desk — meaningless on the device it is being sent
    /// to. Whoever receives it pastes it into an app that already knows which server
    /// it is talking to.
    func invite(to list: List, as role: Role = .editor) async throws -> String {
        struct Invitation: Decodable { let token: String }

        let data = try await send(
            "POST",
            "/api/lists/\(list.id)/members/invites",
            ["role": role.rawValue]
        )
        do {
            return try Self.decoder.decode(Invitation.self, from: data).token
        } catch {
            throw APIError.transport(error)
        }
    }

    /// Withdraws every outstanding link to this list. The only revocation there is:
    /// an owner never sees a link again and cannot tell one from another.
    func revokeInvites(to list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/members/invites", nil)
    }

    /// Follows a link. Answers with the list, so a caller can go straight to it.
    @discardableResult
    func join(withToken token: String) async throws -> List {
        // In the body, not the path. A path is what a proxy or an access log writes
        // down, and a share token is a credential that stays valid for a week.
        let data = try await send("POST", "/api/invites", ["token": token])
        do {
            return try Self.decoder.decode(List.self, from: data)
        } catch {
            throw APIError.transport(error)
        }
    }

    /// Takes somebody off a list. Yourself, which is leaving, or somebody else, which
    /// only an owner may do.
    func remove(_ person: Person, from list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/members/\(person.userID)", nil)
    }

    // MARK: - Lists

    /// Makes a list. The server answers with it, role included.
    @discardableResult
    func createList(named name: String) async throws -> List {
        let data = try await send("POST", "/api/lists", ["name": name])
        do {
            return try Self.decoder.decode(List.self, from: data)
        } catch {
            throw APIError.transport(error)
        }
    }

    func rename(_ list: List, to name: String) async throws {
        _ = try await send("PUT", "/api/lists/\(list.id)", ["name": name])
    }

    /// Deletes a list and everything on it. Owner only; the service refuses anyone
    /// else, and the screens do not offer it to them.
    func delete(_ list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)", nil)
    }

    /// Adds an item from one typed line.
    ///
    /// Sent under `line`, not `name`: `name` is taken literally, and `line` is read
    /// the way a person means it. The parsing happens on the server, so "2 kg apples"
    /// means the same thing here as in the browser -- doing it twice, in two
    /// languages, is how the two come to disagree.
    func add(_ line: String, to list: List) async throws {
        _ = try await send("POST", "/api/lists/\(list.id)/items", ["line": line])
    }

    /// `at` is accepted and not sent: the route is a bare POST with no body. A queued
    /// tick carries its time through the sync route instead -- see `SyncOperation.at` --
    /// and that is the path a tick made out of signal actually takes.
    func setDone(_ item: Item, on list: List, done: Bool, at: Date) async throws {
        try await setDone(itemID: item.id, listID: list.id, done: done)
    }

    /// The same call, by id.
    ///
    /// What the outbox replays holds ids rather than rows: the row it was made against
    /// may have changed three times since, and the operation is about the item, not
    /// about the copy of it that happened to be on screen.
    func setDone(itemID: Int64, listID: Int64, done: Bool) async throws {
        let path = "/api/lists/\(listID)/items/\(itemID)/done"
        _ = try await send(done ? "POST" : "DELETE", path, nil)
    }

    /// Corrects an item. The whole item goes back, because the route is a PUT.
    func update(
        _ item: Item,
        on list: List,
        name: String,
        amount: Double,
        unitID: Int64?
    ) async throws {
        // NSNull rather than a missing key: leaving `unit_id` out means "no opinion"
        // to some servers and this one takes it as null anyway, but a nil Optional put
        // straight into the dictionary makes JSONSerialization throw, and `send` drops
        // a body it cannot encode without a word. Clearing a unit has to be explicit.
        let body: [String: Any] = [
            "name": name,
            "amount": amount,
            "unit_id": unitID.map { $0 as Any } ?? NSNull(),
        ]
        _ = try await send("PUT", "/api/lists/\(list.id)/items/\(item.id)", body)
    }

    func attach(_ tag: Tag, to item: Item, on list: List) async throws {
        _ = try await send(
            "POST",
            "/api/lists/\(list.id)/items/\(item.id)/tags",
            ["tag_id": tag.id]
        )
    }

    func detach(_ tag: Tag, from item: Item, on list: List) async throws {
        _ = try await send(
            "DELETE",
            "/api/lists/\(list.id)/items/\(item.id)/tags/\(tag.id)",
            nil
        )
    }

    /// Empties the trolley: everything ticked off, in one request.
    func clearDone(on list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/items/done", nil)
    }

    func delete(_ item: Item, on list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/items/\(item.id)", nil)
    }

    // MARK: - Watching

    /// A stream that yields when the set of lists this person can see changes.
    ///
    /// A separate stream from a list's own, because it answers a different question:
    /// a list that has just been made has no watchers, so announcing it to itself
    /// reaches nobody.
    func listChanges() async throws -> AsyncThrowingStream<Void, Error> {
        try await stream(at: "/api/me/events")
    }

    /// A stream that yields once each time this list changes anywhere.
    ///
    /// It yields nothing but the fact of the change. Carrying the rows would make the
    /// phone a second source of truth for order and content, and one dropped event
    /// would leave it confidently disagreeing with the browser; a screen that is only
    /// told "re-read" cannot drift.
    ///
    /// Authorised once, when the connection opens. A token that expires mid-stream
    /// does not close it, and does not need to: the stream carries no list content,
    /// and every actual read still presents a fresh token.
    /// A server's per-list events, which are all about the rows.
    ///
    /// `domain` announces on this channel for items and for filing -- see
    /// `service::tags::attach`. It does **not** announce when a category is created,
    /// renamed, removed or reordered, so those do not arrive here and cannot be
    /// reported as `.categories`. What covers that gap is the local half of
    /// `CachingBackend.changes(on:)`, which sees this device's own edits, and
    /// `LocalBackend`, which says so itself.
    func changes(on list: List) async throws -> AsyncThrowingStream<Nudge, Error> {
        let events = try await stream(at: "/api/lists/\(list.id)/events")
        return AsyncThrowingStream { continuation in
            let pump = Task {
                do {
                    for try await _ in events { continuation.yield(.rows) }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in pump.cancel() }
        }
    }

    /// Opens an event stream and yields once per event.
    ///
    /// Authorised once, when the connection opens. A token that expires mid-stream
    /// does not close it, and does not need to: the stream carries no content, and
    /// every actual read still presents a fresh token.
    private func stream(at path: String) async throws -> AsyncThrowingStream<Void, Error> {
        guard let url = URL(string: baseURL.absoluteString + path) else {
            throw APIError.badInput("Bad address for events")
        }

        var request = URLRequest(url: url)
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        // The default is 60 seconds of silence, which would hang up on a quiet list.
        // The server sends a keep-alive comment well inside this.
        request.timeoutInterval = 3600
        guard let token = await token() else { throw noToken() }
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

        let route = Route(path: path)
        let bytes: URLSession.AsyncBytes
        let response: URLResponse
        do {
            (bytes, response) = try await session.bytes(for: request)
        } catch {
            // Warn rather than info: a stream that will not open is the difference
            // between a list that updates itself and one that does not, and it is
            // invisible on screen -- the app looks exactly right and is simply late for
            // ever. This is the log line that tells the two apart.
            Log.warn(
                .stream, "could not open the change stream",
                Detail("route", .route(route)),
                Detail("why", .failure(Plain.Failure(error)))
            )
            Metrics.shared.count(
                Measured.streamState,
                Tagged("route", .route(route)),
                Tagged("state", .outcome(.unreachable))
            )
            throw APIError.transport(error)
        }

        guard let http = response as? HTTPURLResponse else { throw APIError.server(0) }
        let status = http.statusCode
        guard (200..<300).contains(status) else {
            Log.warn(
                .stream, "the server would not open a change stream",
                Detail("route", .route(route)),
                Detail("outcome", .request(RequestOutcome(status: status)))
            )
            Metrics.shared.count(
                Measured.streamState,
                Tagged("route", .route(route)),
                Tagged("state", .request(RequestOutcome(status: status)))
            )
            switch status {
            case 401: throw APIError.unauthorized
            case 403: throw APIError.forbidden
            case 404: throw APIError.notFound
            case let code: throw APIError.server(code)
            }
        }

        Log.info(.stream, "the change stream is open", Detail("route", .route(route)))
        Metrics.shared.count(
            Measured.streamState,
            Tagged("route", .route(route)),
            Tagged("state", .outcome(.ok))
        )

        return AsyncThrowingStream { continuation in
            let reading = Task {
                var nudges = 0
                do {
                    for try await line in bytes.lines {
                        // Keep-alives arrive as comments (":" first) and event names
                        // as "event:", neither of which is news. A "data:" line is.
                        if line.hasPrefix("data:") {
                            nudges += 1
                            Metrics.shared.count(
                                Measured.streamNudge,
                                Tagged("route", .route(route))
                            )
                            continuation.yield(())
                        }
                    }
                    // A stream that ends without an error has been hung up on, which is
                    // ordinary -- a proxy timeout, a phone going to sleep -- and is still
                    // worth a line, because "the list stopped updating" and "the stream
                    // closed an hour ago" are the same event seen from two ends.
                    Log.info(
                        .stream, "the change stream closed",
                        Detail("route", .route(route)),
                        Detail("nudges", .count(nudges)),
                        Detail("outcome", .outcome(.ok))
                    )
                    Metrics.shared.count(
                        Measured.streamState,
                        Tagged("route", .route(route)),
                        Tagged("state", .outcome(.cancelled))
                    )
                    continuation.finish()
                } catch {
                    Log.warn(
                        .stream, "the change stream dropped",
                        Detail("route", .route(route)),
                        Detail("nudges", .count(nudges)),
                        Detail("why", .failure(Plain.Failure(error)))
                    )
                    Metrics.shared.count(
                        Measured.streamState,
                        Tagged("route", .route(route)),
                        Tagged("state", .outcome(.unreachable))
                    )
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in reading.cancel() }
        }
    }

    /// Replays everything this device did while it could not reach the server.
    ///
    /// One request for the batch, and one answer per operation. Nothing here decides
    /// what an answer means — see ``Outbox/drain(through:)``, which is the only caller.
    func sync(_ operations: [SyncOperation]) async throws -> [AppliedOperation] {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601

        guard let body = try? encoder.encode(SyncBatch(operations: operations)) else {
            throw APIError.badInput("Could not describe what is queued.")
        }

        let data = try await sendRaw("POST", "/api/sync", body)
        do {
            return try Self.decoder.decode(Replayed.self, from: data).operations
        } catch {
            throw APIError.transport(error)
        }
    }

    // MARK: - Plumbing

    private func get<T: Decodable>(_ path: String) async throws -> T {
        let data = try await send("GET", path, nil)
        do {
            return try Self.decoder.decode(T.self, from: data)
        } catch {
            throw APIError.transport(error)
        }
    }

    /// The same request as ``send(_:_:_:)`` with a body already encoded.
    ///
    /// `send` builds its JSON from a dictionary, which is fine for the small bodies the
    /// REST routes take and wrong for a batch of operations — those have a shape worth
    /// keeping in a type.
    private func sendRaw(_ method: String, _ path: String, _ body: Data) async throws -> Data {
        try await send(method, path, nil, body)
    }

    /// Every request, timed and counted.
    ///
    /// Wrapped around ``perform(_:_:_:_:)`` rather than written into it so there is one
    /// place a request can end — six `throw`s and one `return` were seven places to
    /// remember to record, and the ones that would have been forgotten are the failures,
    /// which are the interesting half.
    ///
    /// What is recorded is a ``Route`` and a ``RequestOutcome``: a class of request and
    /// one of five endings. Never the path -- see `Route`, which exists because
    /// `/api/lists/41/items` names a list and a share token travels in one too.
    @discardableResult
    private func send(
        _ method: String,
        _ path: String,
        _ body: [String: Any]?,
        _ encoded: Data? = nil
    ) async throws -> Data {
        let route = Route(path: path)
        let began = DispatchTime.now()

        do {
            let data = try await perform(method, path, body, encoded)
            note(route, .ok, since: began, bytes: data.count)
            return data
        } catch {
            note(route, RequestOutcome(error: error), since: began, bytes: 0)
            throw error
        }
    }

    /// Writes down what one request cost and how it ended.
    ///
    /// `info` and below, so it is silent until somebody turns logging on -- a phone
    /// scrolling a list makes a request per screen and this would otherwise be the
    /// loudest thing in the file.
    private func note(
        _ route: Route,
        _ outcome: RequestOutcome,
        since began: DispatchTime,
        bytes: Int
    ) {
        let took = Double(DispatchTime.now().uptimeNanoseconds - began.uptimeNanoseconds) / 1_000_000

        Log.info(
            .backend, "asked the server",
            Detail("route", .route(route)),
            Detail("outcome", .request(outcome)),
            Detail("took", .milliseconds(Int(took))),
            Detail("bytes", .count(bytes))
        )
        Metrics.shared.observe(
            Measured.requestDuration,
            milliseconds: took,
            Tagged("route", .route(route)),
            Tagged("outcome", .request(outcome))
        )
        Metrics.shared.count(
            Measured.requests,
            Tagged("route", .route(route)),
            Tagged("outcome", .request(outcome))
        )
    }

    @discardableResult
    private func perform(
        _ method: String,
        _ path: String,
        _ body: [String: Any]?,
        _ encoded: Data? = nil
    ) async throws -> Data {
        guard reachable else { throw APIError.transport(NoServer()) }

        guard let url = URL(string: baseURL.absoluteString + path) else {
            throw APIError.badInput("Bad address: \(path)")
        }

        var request = URLRequest(url: url)
        request.httpMethod = method

        guard let token = await token() else { throw noToken() }
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

        if let encoded {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = encoded
        } else if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try? JSONSerialization.data(withJSONObject: body)
        }

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw APIError.transport(error)
        }

        guard let http = response as? HTTPURLResponse else {
            throw APIError.server(0)
        }

        switch http.statusCode {
        case 200..<300:
            return data
        case 401:
            throw APIError.unauthorized
        case 403:
            throw Self.refusal(from: data)
        case 404:
            throw APIError.notFound
        case 400, 409, 422:
            throw APIError.badInput(Self.message(from: data) ?? "The server would not accept that.")
        case let code:
            throw APIError.server(code)
        }
    }

    /// Which of the two 403s this is.
    ///
    /// The body carries `"reason": "not_admitted"` for the one that is about the
    /// account rather than about a list. An older server sends no `reason` at all,
    /// and the safe reading of silence is the narrower refusal.
    private static func refusal(from data: Data) -> APIError {
        guard
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            object["reason"] as? String == "not_admitted"
        else { return .forbidden }
        return .notAdmitted
    }

    /// What a missing token means, which depends on whether anybody is signed in.
    private func noToken() -> APIError {
        guard remembered() else { return .unauthorized }
        return .transport(
            NSError(
                domain: NSURLErrorDomain,
                code: NSURLErrorNotConnectedToInternet,
                userInfo: [NSLocalizedDescriptionKey: "No connection to sign in with."]
            )
        )
    }

    /// The API answers errors as `{"error": "..."}`.
    private static func message(from data: Data) -> String? {
        guard
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let error = object["error"] as? String
        else { return nil }
        return error
    }

    private static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        // The API serialises timestamps as RFC 3339, which is what `.iso8601` reads.
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}

/// There is no server, because somebody said so.
///
/// Its own error type rather than a string, so that the sentence a screen shows is
/// decided by the screen. What matters here is only that it arrives as a transport
/// failure, which the app already treats as "show the cache and keep the queue".
struct NoServer: LocalizedError {
    var errorDescription: String? { "This device is not using a server." }
}
