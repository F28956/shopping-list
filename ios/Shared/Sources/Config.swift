import Foundation

/// Where the server is.
///
/// From the bundle rather than a constant, so pointing the app at a different machine
/// is a build setting. Both the phone and the watch read their own copy, built from
/// the same `Config.xcconfig`, so there is nothing to keep in step at runtime.
///
/// `localhost` is the device itself, which is the first thing to get wrong once this
/// leaves the simulator.
enum Config {
    static var apiBaseURL: URL {
        let raw = Bundle.main.object(forInfoDictionaryKey: "ShoppingListAPIBaseURL") as? String
        return URL(string: raw ?? "") ?? URL(string: "http://localhost:8080")!
    }
}
