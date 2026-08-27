import Foundation
import Security

/// The watch's copy of the token, on disk rather than in memory.
///
/// A duplicate of `Keychain` in `Auth/Sources` rather than a shared file, because the
/// watch target deliberately does not link `Auth` — there is no sign-in on a watch and
/// nothing there it could use. Sharing it would mean the watch built against a flow it
/// cannot run, to reuse forty lines.
enum WatchKeychain {
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

    static func set(_ value: String?, for key: String) {
        SecItemDelete(base(key) as CFDictionary)

        guard let value, let data = value.data(using: .utf8) else { return }

        var query = base(key)
        query[kSecValueData as String] = data
        // After first unlock, so a complication refreshing on a wrist that has not
        // been raised still has something to send.
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
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
