import Foundation

/// What went wrong, in terms the screen can answer.
enum APIError: LocalizedError {
    /// The server did not accept the token. Usually means signed out, or that the
    /// server has not been told about this app's client id.
    case unauthorized
    case notFound
    /// The service refused: a viewer trying to change something.
    case forbidden
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
        self.session = URLSession(configuration: .default)
        self.token = token
    }

    // MARK: - Reading

    func lists() async throws -> [List] {
        let page: Page<List> = try await get("/api/lists?order_by=updated_at&direction=descending")
        return page.items
    }

    func items(on list: List) async throws -> [Item] {
        // Outstanding first, then what is already in the trolley — the same order the
        // web UI uses, so the two do not show the same list differently.
        let page: Page<Item> = try await get(
            "/api/lists/\(list.id)/items?order_by=done_at&direction=ascending&size=200"
        )
        return page.items
    }

    /// Every unit, in name order.
    ///
    /// An array rather than a lookup because the editor's picker needs an order and a
    /// dictionary has none. Rows still want the lookup, and the screen builds it.
    func units() async throws -> [Unit] {
        let page: Page<Unit> = try await get("/api/units?order_by=name&size=200")
        return page.items
    }

    // MARK: - Writing

    /// Adds an item from one typed line.
    ///
    /// The line is sent as typed and parsed on the server, so "2 kg apples" means the
    /// same thing here as in the browser. Parsing it twice, in two languages, is how
    /// the two come to disagree.
    func add(_ line: String, to list: List) async throws {
        _ = try await send("POST", "/api/lists/\(list.id)/items", ["name": line])
    }

    func setDone(_ item: Item, on list: List, done: Bool) async throws {
        let path = "/api/lists/\(list.id)/items/\(item.id)/done"
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

    /// Empties the trolley: everything ticked off, in one request.
    func clearDone(on list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/items/done", nil)
    }

    func delete(_ item: Item, on list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/items/\(item.id)", nil)
    }

    // MARK: - Watching

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
        guard let url = URL(string: "/api/lists/\(list.id)/events", relativeTo: baseURL) else {
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
                        // Keep-alives arrive as comments (":" first) and event names as
                        // "event:", neither of which is news. A "data:" line is.
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

    // MARK: - Plumbing

    private func get<T: Decodable>(_ path: String) async throws -> T {
        let data = try await send("GET", path, nil)
        do {
            return try Self.decoder.decode(T.self, from: data)
        } catch {
            throw APIError.transport(error)
        }
    }

    @discardableResult
    private func send(_ method: String, _ path: String, _ body: [String: Any]?) async throws -> Data {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw APIError.badInput("Bad address: \(path)")
        }

        var request = URLRequest(url: url)
        request.httpMethod = method

        guard let token = await token() else { throw APIError.unauthorized }
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

        if let body {
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
            throw APIError.forbidden
        case 404:
            throw APIError.notFound
        case 400, 409, 422:
            throw APIError.badInput(Self.message(from: data) ?? "The server would not accept that.")
        case let code:
            throw APIError.server(code)
        }
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
