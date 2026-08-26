#if DEBUG

    import Foundation

    /// Answers the app's requests from `StubWorld` instead of the network.
    ///
    /// A `URLProtocol` rather than a fake `API`: this way the real `API` actor runs —
    /// its URLs, its headers, its status handling, its JSON decoding — and only the
    /// wire underneath it is replaced. A test that passes here has exercised the
    /// decoding path that would otherwise be the likeliest thing to break.
    final class StubURLProtocol: URLProtocol {
        override class func canInit(with request: URLRequest) -> Bool { true }

        override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

        override func startLoading() {
            guard let url = request.url else { return finish(status: 400, body: "{}") }
            let path = url.path
            let method = request.httpMethod ?? "GET"
            let world = StubWorld.shared

            // An event stream that never says anything. Ending it would send the app
            // into its reconnect loop, which reloads — and a screen quietly reloading
            // underneath a test is how a test becomes flaky.
            if path.hasSuffix("/events") { return }

            switch (method, path) {
            case ("GET", "/api/lists"):
                finish(status: 200, body: world.listsJSON())
            case ("POST", "/api/lists"):
                finish(status: 201, body: world.createList(named: body()["name"] as? String ?? ""))
            case ("GET", "/api/units"):
                finish(status: 200, body: world.unitsJSON())
            case ("GET", "/api/tags"):
                finish(status: 200, body: world.tagsJSON())
            case ("GET", let p) where p.hasSuffix("/history"):
                let typed = URLComponents(url: url, resolvingAgainstBaseURL: false)?
                    .queryItems?.first { $0.name == "q" }?.value ?? ""
                finish(status: 200, body: world.historyJSON(matching: typed))
            case ("GET", let p) where p.hasSuffix("/items"):
                finish(status: 200, body: world.itemsJSON(list: onList(p)))
            case ("GET", let p) where p.hasSuffix("/tags"):
                finish(status: 200, body: world.tagsOnItemJSON(itemID(from: p) ?? 0))

            case ("POST", let p) where p.hasSuffix("/done"):
                world.setDone(itemID(from: p) ?? 0, true)
                finish(status: 200, body: "{}")
            case ("DELETE", let p) where p.hasSuffix("/items/done"):
                world.clearDone()
                finish(status: 200, body: #"{"cleared": 1}"#)
            case ("DELETE", let p) where p.hasSuffix("/done"):
                world.setDone(itemID(from: p) ?? 0, false)
                finish(status: 200, body: "{}")

            case ("POST", let p) where p.hasSuffix("/items"):
                world.add(line: body()["line"] as? String ?? "", to: onList(p))
                finish(status: 201, body: "{}")
            case ("PUT", let p) where listID(from: p) != nil && !p.contains("/items"):
                world.renameList(listID(from: p)!, to: body()["name"] as? String ?? "")
                finish(status: 200, body: "{}")
            case ("DELETE", let p) where listID(from: p) != nil && !p.contains("/items"):
                world.deleteList(listID(from: p)!)
                finish(status: 204, body: "")

            case ("PUT", let p):
                let sent = body()
                world.update(
                    itemID(from: p) ?? 0,
                    name: sent["name"] as? String ?? "",
                    amount: sent["amount"] as? Double ?? 1,
                    unitID: sent["unit_id"] as? Int64
                )
                finish(status: 200, body: "{}")

            case ("POST", let p) where p.hasSuffix("/tags"):
                world.attach(Int64(body()["tag_id"] as? Int ?? 0), to: itemID(from: p) ?? 0)
                finish(status: 204, body: "")
            case ("DELETE", let p) where p.contains("/tags/"):
                let parts = p.split(separator: "/")
                world.detach(Int64(parts.last ?? "0") ?? 0, from: itemID(from: p) ?? 0)
                finish(status: 204, body: "")
            case ("DELETE", let p):
                world.delete(itemID(from: p) ?? 0)
                finish(status: 204, body: "")

            default:
                finish(status: 404, body: #"{"error": "Not Found"}"#)
            }
        }

        override func stopLoading() {}

        /// The list a nested path belongs to: the `1` in `/api/lists/1/items/...`.
        private func onList(_ path: String) -> Int64 {
            let parts = path.split(separator: "/")
            guard let at = parts.firstIndex(of: "lists"), parts.indices.contains(at + 1)
            else { return 1 }
            return Int64(parts[at + 1]) ?? 1
        }

        /// The list id out of `/api/lists/1`, and only when that is the whole path —
        /// `/api/lists/1/items/7` is an item's business, not a list's.
        private func listID(from path: String) -> Int64? {
            let parts = path.split(separator: "/")
            guard parts.count == 3, parts[1] == "lists" else { return nil }
            return Int64(parts[2])
        }

        /// The item id out of `/api/lists/1/items/7/...`.
        private func itemID(from path: String) -> Int64? {
            let parts = path.split(separator: "/")
            guard let at = parts.firstIndex(of: "items"), parts.indices.contains(at + 1)
            else { return nil }
            return Int64(parts[at + 1])
        }

        private func body() -> [String: Any] {
            // `httpBody` is nil for a request built through URLSession's upload path,
            // where the bytes are on the stream instead.
            let data = request.httpBody ?? request.httpBodyStream.map(Self.drain) ?? Data()
            return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
        }

        private static func drain(_ stream: InputStream) -> Data {
            stream.open()
            defer { stream.close() }

            var data = Data()
            let size = 4096
            var buffer = [UInt8](repeating: 0, count: size)
            while stream.hasBytesAvailable {
                let read = stream.read(&buffer, maxLength: size)
                if read <= 0 { break }
                data.append(buffer, count: read)
            }
            return data
        }

        private func finish(status: Int, body: String) {
            let response = HTTPURLResponse(
                url: request.url!,
                statusCode: status,
                httpVersion: "HTTP/1.1",
                headerFields: ["Content-Type": "application/json"]
            )!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: Data(body.utf8))
            client?.urlProtocolDidFinishLoading(self)
        }
    }

#endif
