import Testing
import Foundation
@testable import ShoppingList

/// Reading the token out of whatever somebody pastes.
struct JoinLinkTests {
    @Test(arguments: [
        ("http://localhost:8080/join/abc123", "abc123"),
        ("https://shopping.example/join/abc123", "abc123"),
        // pasted with the whitespace a chat app leaves behind
        ("  http://localhost:8080/join/abc123 \n", "abc123"),
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
    ])
    func theseAreNotLinks(_ pasted: String) {
        #expect(token(in: pasted) == nil, "\(pasted) was read as a token")
    }
}
