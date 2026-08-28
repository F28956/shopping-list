import Foundation
import Testing

@testable import ShoppingList

/// Asking an address whether it is a Shopping List server.
///
/// The point of C2 is that the three ways this fails are fixed three different ways, so
/// the tests are mostly about telling them apart rather than about the happy path.
struct ServerDirectoryTests {
    /// Answers with whatever the test set, so the real `URLSession` path runs and only
    /// the wire underneath is replaced.
    final class Canned: URLProtocol, @unchecked Sendable {
        nonisolated(unsafe) static var status = 200
        nonisolated(unsafe) static var body = ""
        nonisolated(unsafe) static var failure: URLError?

        override class func canInit(with request: URLRequest) -> Bool { true }
        override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
        override func stopLoading() {}

        override func startLoading() {
            if let failure = Self.failure {
                client?.urlProtocol(self, didFailWithError: failure)
                return
            }

            let response = HTTPURLResponse(
                url: request.url!,
                statusCode: Self.status,
                httpVersion: nil,
                headerFields: nil
            )!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: Data(Self.body.utf8))
            client?.urlProtocolDidFinishLoading(self)
        }
    }

    private func session() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [Canned.self]
        return URLSession(configuration: configuration)
    }

    private func asking(
        status: Int = 200,
        body: String = "",
        failure: URLError? = nil
    ) async -> Result<ServerDirectory.About, ServerDirectory.Refusal> {
        Canned.status = status
        Canned.body = body
        Canned.failure = failure
        defer { Canned.failure = nil }

        let address = try! ServerAddress.parse("https://example.com").get()
        return await ServerDirectory.ask(address, using: session())
    }

    @Test func aServerThatNamesItselfIsAccepted() async {
        let answer = await asking(
            body: #"{"name":"shopping-list","version":"0.1.0","admission":"closed"}"#
        )

        #expect(
            (try? answer.get())
                == ServerDirectory.About(
                    name: "shopping-list", version: "0.1.0", admission: "closed"
                )
        )
    }

    /// The case `GET /healthz` could not catch. Something is running there and it is
    /// somebody else's.
    @Test func somethingElseAnsweringIsNotThisServer() async {
        let other = await asking(body: #"{"name":"grafana","version":"11","admission":"open"}"#)
        #expect((try? other.get()) == nil)

        let nonsense = await asking(body: "ok")
        #expect((try? nonsense.get()) == nil)

        let missing = await asking(status: 404, body: "not found")
        #expect((try? missing.get()) == nil)
    }

    /// Told apart because it is fixed differently: a certificate is repaired on the
    /// server, and a wrong address is retyped.
    @Test func aRefusedCertificateSaysSo() async {
        let answer = await asking(failure: URLError(.serverCertificateUntrusted))

        guard case .failure(let refusal) = answer else {
            Issue.record("a bad certificate was accepted")
            return
        }
        #expect(refusal == .certificateRefused)
    }

    @Test func nothingThereIsItsOwnAnswer() async {
        let answer = await asking(failure: URLError(.cannotConnectToHost))

        guard case .failure(let refusal) = answer else {
            Issue.record("an unreachable address was accepted")
            return
        }
        #expect(refusal == .unreachable)
    }

    /// The sign-in screen reads these to decide whether to offer a claim, and whether
    /// to warn that you will need to be let in.
    @Test func admissionIsReadableByTheSignInScreen() {
        func about(_ admission: String) -> ServerDirectory.About {
            ServerDirectory.About(name: "shopping-list", version: "1", admission: admission)
        }

        #expect(about("unclaimed").isUnclaimed)
        #expect(!about("closed").isUnclaimed)
        #expect(about("open").admitsAnyone)
        #expect(!about("closed").admitsAnyone)
    }

    /// On a build compiled with an address, forgetting must not fall back to it —
    /// that would be a "stop using this server" button that appears to work and does
    /// nothing.
    @Test func forgettingLeavesTheDeviceOnItsOwn() {
        let key = "server.address"
        let before = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(before, forKey: key) }

        ServerDirectory.remember(try! ServerAddress.parse("https://one.example.com").get())
        ServerDirectory.forget()

        #expect(ServerDirectory.current == nil)
        #expect(ServerDirectory.isOnDeviceOnly)
    }

    /// The default, and the reason there is no first-run screen: a shopping list opens
    /// and is usable, rather than opening and asking a question about hosting.
    @Test func sayingNothingMeansThisDeviceOnly() {
        let key = "server.address"
        let before = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(before, forKey: key) }

        // **Absent**, and asserted as absent. This used to remove the key and then
        // write an empty string before asserting, with a note that a debug build is
        // pointed at the machine on the desk and is "the one exception" -- which had
        // stopped being true when `choice` stopped consulting the build setting. So the
        // one case that matters, a cold start on a phone nobody has configured, was the
        // one case not covered.
        UserDefaults.standard.removeObject(forKey: key)

        #expect(ServerDirectory.isOnDeviceOnly)
        #expect(ServerDirectory.current == nil)
    }

    /// The build setting is not a default, and a fresh install must not adopt it.
    ///
    /// It is written down so a debug build can reach the server on the same desk. Read
    /// as a fallback it made "the app opens straight into a usable list" true of a
    /// release build and false of every build anybody runs -- and, worse, it is a
    /// suggestion about where somebody's shopping should live.
    @Test func aBuildSettingIsNotAServerSomebodyChose() {
        let key = "server.address"
        let before = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(before, forKey: key) }
        UserDefaults.standard.removeObject(forKey: key)

        #expect(
            ServerDirectory.current?.origin != Config.apiBaseURL.absoluteString,
            "a fresh install adopted the address the build was compiled with"
        )
        #expect(Capabilities.current == .onItsOwn, "and it offered sharing and accounts")
    }

    @Test func everyRefusalSaysSomething() {
        for refusal: ServerDirectory.Refusal in [.unreachable, .notThisSoftware, .certificateRefused] {
            #expect(!refusal.sentence.isEmpty)
        }
    }

    /// Changing servers is what tells the caller to throw the cache away, so getting
    /// this answer wrong shows one server's lists under another server's name.
    @Test func rememberingSaysWhetherTheServerChanged() {
        let key = "server.address"
        let before = UserDefaults.standard.string(forKey: key)
        defer { UserDefaults.standard.set(before, forKey: key) }

        let one = try! ServerAddress.parse("https://one.example.com").get()
        let two = try! ServerAddress.parse("https://two.example.com").get()

        ServerDirectory.remember(one)
        #expect(ServerDirectory.current == one)
        #expect(ServerDirectory.remember(one) == false, "the same server read as a change")
        #expect(ServerDirectory.remember(two) == true, "a different server read as the same")
        #expect(ServerDirectory.current == two)
    }
}
