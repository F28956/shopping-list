import Testing
import Foundation
@testable import ShoppingList

/// Reading the token out of whatever somebody pastes.
struct JoinLinkTests {
    @Test(arguments: [
        // The shape a server issues: the token is in the fragment, where no proxy
        // and no access log between here and somebody's home server ever sees it.
        ("http://localhost:8080/join#abc123", "abc123"),
        ("https://shopping.example/join#abc123", "abc123"),
        // pasted with the whitespace a chat app leaves behind
        ("  http://localhost:8080/join#abc123 \n", "abc123"),
        // The older shape, with the token in the path. Still read, so that a link
        // sent before a server was updated does not stop working in somebody's inbox.
        ("http://localhost:8080/join/abc123", "abc123"),
        ("https://shopping.example/join/abc123", "abc123"),
        // just the token, which is what somebody who read the link sends on
        ("abc123", "abc123"),
        ("  abc123  ", "abc123"),
    ])
    func aTokenIsFound(pasted: String, expected: String) {
        #expect(token(in: pasted) == expected)
    }

    @Test(arguments: [
        "",
        "   ",
        // a sentence, not a link: somebody pasted the whole message
        "here is the link I promised",
        // a link to nothing in particular
        "http://localhost:8080/",
        // the join page with no invitation on it
        "http://localhost:8080/join",
        "http://localhost:8080/join#",
    ])
    func theseAreNotLinks(_ pasted: String) {
        #expect(token(in: pasted) == nil, "\(pasted) was read as a token")
    }
}

    // MARK: - The origin a link carries (C7)

    @Test(arguments: [
        ("https://shopping.example/join#abc123", "https://shopping.example"),
        ("http://localhost:8080/join#abc123", "http://localhost:8080"),
        ("  https://Shop.Example.com:8443/join/abc \n", "https://shop.example.com:8443"),
        // A default port is noise, and `ServerAddress` is the one place that decides.
        ("https://shopping.example:443/join/abc", "https://shopping.example"),
    ])
    func aLinkNamesItsServer(pasted: String, expected: String) {
        #expect(server(in: pasted)?.origin == expected)
    }

    /// A bare token names nothing, so there is nothing to offer — which is the case
    /// where the app must go on asking.
    @Test(arguments: ["abc123", "  abc123  ", "", "here is the link I promised"])
    func theseNameNoServer(_ pasted: String) {
        #expect(server(in: pasted) == nil, "\(pasted) was read as a server")
    }
