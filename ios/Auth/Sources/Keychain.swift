import Foundation
import Security

/// Where the session token lives.
///
/// `UserDefaults` holds the fact that somebody is signed in and what to call them;
/// this holds the thing that actually grants access. The difference matters because a
/// device backup, a crash log and a screen recording all reach a plist, and none of
/// them reach here.
///
/// Deliberately tiny. One string under one name, no groups and no sharing: the watch
/// gets its token by asking the phone over WatchConnectivity, which is a link Apple
/// already authenticates, rather than by sharing a keychain across a pairing.
enum Keychain {
    /// `kSecAttrAccessibleAfterFirstUnlock`, so a background refresh on a locked phone
    /// still has a token. `WhenUnlocked` would mean the app woke, found nothing, and
    /// decided it was signed out.
    private static let accessibility = kSecAttrAccessibleAfterFirstUnlock

    static func string(for key: String) -> String? {
        var query = base(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne

        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data
        else { return nil }

        return String(data: data, encoding: .utf8)
    }

    /// Writes, replacing whatever was there.
    ///
    /// Delete-then-add rather than `SecItemUpdate`: the update path has to know
    /// whether the item exists, and getting that wrong leaves a stale token behind a
    /// failed write — which is the one outcome that would show as "signed in as
    /// somebody else".
    static func set(_ value: String?, for key: String) {
        SecItemDelete(base(key) as CFDictionary)

        guard let value, let data = value.data(using: .utf8) else { return }

        var query = base(key)
        query[kSecValueData as String] = data
        query[kSecAttrAccessible as String] = accessibility
        SecItemAdd(query as CFDictionary, nil)
    }

    private static func base(_ key: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "shopping-list",
            kSecAttrAccount as String: key,
        ]
    }
}
