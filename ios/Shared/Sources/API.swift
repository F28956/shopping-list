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
    private let baseURL: URL
    private let session: URLSession
    private let token: () async -> String?

    init(baseURL: URL, token: @escaping () async -> String?) {
        self.baseURL = baseURL
        self.token = token

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

    // MARK: - Writing

    // MARK: - Sharing

    /// Who this is, so a screen can tell which member is you.
    func whoAmI() async throws -> Me {
        try await get("/api/me")
    }

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
        let data = try await send("POST", "/api/invites/\(token)", nil)
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

    func setDone(_ item: Item, on list: List, done: Bool) async throws {
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
    func changes(on list: List) async throws -> AsyncThrowingStream<Void, Error> {
        try await stream(at: "/api/lists/\(list.id)/events")
    }

    /// Opens an event stream and yields once per event.
    ///
    /// Authorised once, when the connection opens. A token that expires mid-stream
    /// does not close it, and does not need to: the stream carries no content, and
    /// every actual read still presents a fresh token.
    private func stream(at path: String) async throws -> AsyncThrowingStream<Void, Error> {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw APIError.badInput("Bad address for events")
        }

        var request = URLRequest(url: url)
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
        // The default is 60 seconds of silence, which would hang up on a quiet list.
        // The server sends a keep-alive comment well inside this.
        request.timeoutInterval = 3600
        guard let token = await token() else { throw APIError.unauthorized }
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

        let bytes: URLSession.AsyncBytes
        let response: URLResponse
        do {
            (bytes, response) = try await session.bytes(for: request)
        } catch {
            throw APIError.transport(error)
        }

        guard let http = response as? HTTPURLResponse else { throw APIError.server(0) }
        switch http.statusCode {
        case 200..<300: break
        case 401: throw APIError.unauthorized
        case 403: throw APIError.forbidden
        case 404: throw APIError.notFound
        case let code: throw APIError.server(code)
        }

        return AsyncThrowingStream { continuation in
            let reading = Task {
                do {
                    for try await line in bytes.lines {
                        // Keep-alives arrive as comments (":" first) and event names
                        // as "event:", neither of which is news. A "data:" line is.
                        if line.hasPrefix("data:") { continuation.yield(()) }
                    }
                    continuation.finish()
                } catch {
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

    @discardableResult
    private func send(
        _ method: String,
        _ path: String,
        _ body: [String: Any]?,
        _ encoded: Data? = nil
    ) async throws -> Data {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw APIError.badInput("Bad address: \(path)")
        }

        var request = URLRequest(url: url)
        request.httpMethod = method

        guard let token = await token() else { throw APIError.unauthorized }
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
