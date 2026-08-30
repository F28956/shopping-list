import Foundation
import Testing

@testable import ShoppingList

/// What somebody types, and what is stored.
///
/// The cases that matter are not the well-formed ones. They are the paste out of a
/// location bar, the missing scheme, and the trailing slash — see `ServerAddress`.
struct ServerAddressTests {
    private func origin(_ typed: String) -> String? {
        try? ServerAddress.parse(typed).get().origin
    }

    private func problem(_ typed: String) -> ServerAddress.Problem? {
        switch ServerAddress.parse(typed) {
        case .success: nil
        case .failure(let problem): problem
        }
    }

    @Test func anOrdinaryAddressSurvivesUnchanged() {
        #expect(origin("https://shopping.example.com") == "https://shopping.example.com")
    }

    /// The commonest thing typed, and the one `URLComponents` reads as a scheme.
    @Test func aMissingSchemeBecomesHttps() {
        #expect(origin("shopping.example.com") == "https://shopping.example.com")
        #expect(origin("shopping.example.com:8080") == "https://shopping.example.com:8080")
    }

    /// What a browser's location bar shows, and not a path anybody meant.
    @Test func aTrailingSlashGoes() {
        #expect(origin("https://shopping.example.com/") == "https://shopping.example.com")
    }

    @Test func theHostIsLowercasedAndSpaceIgnored() {
        #expect(origin("  HTTPS://Shopping.Example.COM  ") == "https://shopping.example.com")
    }

    /// A path is the prefix the server is mounted under, and is kept.
    ///
    /// It used to be refused. One domain with several things behind it is an ordinary
    /// arrangement, and insisting on a whole host was a constraint on somebody's DNS
    /// rather than a property of this application. The server end is `BASE_PATH`.
    @Test(arguments: [
        ("https://example.com/sl", "https://example.com", "/sl"),
        ("https://example.com/sl/", "https://example.com", "/sl"),
        ("https://example.com/apps/shopping", "https://example.com", "/apps/shopping"),
        ("https://example.com", "https://example.com", ""),
        ("https://example.com/", "https://example.com", ""),
    ])
    func aPathIsKeptAsThePrefix(typed: String, origin: String, prefix: String) throws {
        let address = try ServerAddress.parse(typed).get()

        #expect(address.origin == origin)
        #expect(address.prefix == prefix)
        #expect(address.written == origin + prefix)
    }

    /// A query or a fragment is still refused: neither is beyond doubt.
    @Test func aQueryOrFragmentIsStillRefused() {
        #expect(problem("https://example.com?x=1") == .notJustAnOrigin)
        #expect(problem("https://example.com#top") == .notJustAnOrigin)
        #expect(problem("https://example.com/sl?x=1") == .notJustAnOrigin)
    }

    /// A non-default port is part of the origin; a default one is noise.
    @Test func portsAreKeptOnlyWhenTheySaySomething() {
        #expect(origin("https://example.com:8443") == "https://example.com:8443")
        #expect(origin("https://example.com:443") == "https://example.com")
        #expect(origin("http://example.com:80") == "http://example.com")
        #expect(origin("http://localhost:8080") == "http://localhost:8080")
    }

    /// C6. The alternative is every user's shopping and bearer token in the clear on
    /// whatever café Wi-Fi they are on.
    ///
    /// Which way this goes is decided by the build, not by the caller — there is no
    /// longer a parameter to pass, so no call site can opt itself out. The test says
    /// both halves so that whichever configuration it runs under, it asserts the rule
    /// that configuration is meant to keep.
    @Test func cleartextFollowsTheBuild() {
        if ServerAddress.allowsCleartext {
            #expect(origin("http://example.com") == "http://example.com")
        } else {
            #expect(problem("http://example.com") == .insecure)
        }
        #expect(origin("https://example.com") == "https://example.com")
    }

    @Test func nonsenseIsRefused() {
        #expect(problem("") == .empty)
        #expect(problem("   ") == .empty)
        #expect(problem("ftp://example.com") == .notAnAddress)
        #expect(problem("https://") == .notAnAddress)
        #expect(problem("://nope") == .notAnAddress)
    }

    /// A screen that reaches for `localizedDescription` gets the sentence too.
    ///
    /// The Mac settings window did exactly that, and a bare `Error` renders as its
    /// case index -- so somebody who typed an address with a path on the end was told
    /// "the operation could not be completed, ServerAddress.Problem error 3".
    @Test(arguments: [
        ServerAddress.Problem.empty,
        .notAnAddress,
        .insecure,
        .notJustAnOrigin,
    ])
    func aProblemReadsTheSameWhicheverWayItIsAsked(_ problem: ServerAddress.Problem) {
        #expect(problem.localizedDescription == problem.sentence)
        #expect(!problem.localizedDescription.contains("error"))
    }

    /// Every problem has a sentence, because a screen has to say something.
    @Test func everyProblemSaysSomething() {
        for problem: ServerAddress.Problem in [.empty, .notAnAddress, .insecure, .notJustAnOrigin] {
            #expect(!problem.sentence.isEmpty)
        }
    }

    /// The whole point: one slash between the address and the path, and a prefix that
    /// is still there afterwards.
    ///
    /// Built by joining strings and not by resolving a relative URL.
    /// `URL(string: "/api/lists", relativeTo: "https://example.com/sl")` is
    /// `https://example.com/api/lists` — correct by the RFC, and the prefix is gone.
    @Test(arguments: [
        ("https://example.com/", "https://example.com/api/lists"),
        ("https://example.com/sl", "https://example.com/sl/api/lists"),
        ("https://example.com/apps/shopping", "https://example.com/apps/shopping/api/lists"),
        ("https://example.com:8443/sl", "https://example.com:8443/sl/api/lists"),
    ])
    func aRequestUrlKeepsThePrefix(typed: String, expected: String) throws {
        let address = try ServerAddress.parse(typed).get()

        #expect(address.url(for: "/api/lists")?.absoluteString == expected)
    }
}
