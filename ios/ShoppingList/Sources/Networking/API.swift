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

    func units() async throws -> [Int64: String] {
        let page: Page<Unit> = try await get("/api/units?order_by=name&size=200")
        return Dictionary(uniqueKeysWithValues: page.items.map { ($0.id, $0.name) })
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

    func delete(_ item: Item, on list: List) async throws {
        _ = try await send("DELETE", "/api/lists/\(list.id)/items/\(item.id)", nil)
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
