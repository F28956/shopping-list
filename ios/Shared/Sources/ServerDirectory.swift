import Foundation

/// Which server this device talks to, and how it came to believe that.
///
/// A self-hosted app cannot be built pointed anywhere, because the answer is different
/// for everybody. So the address is stored, and the build setting becomes what a fresh
/// install starts from rather than what it is stuck with.
///
/// Two things live here because they belong together: what is stored, and what it takes
/// to be allowed to store it (C2 — an address is validated by asking it).
enum ServerDirectory {
    private static let key = "server.address"

    /// The address this device uses.
    ///
    /// Falls back to the build setting, so a development build with
    /// `SHOPPING_LIST_API_BASE_URL` in `Config.xcconfig` keeps working with nothing
    /// entered and the simulator keeps talking to `localhost`.
    /// What this device has been told to do about a server.
    enum Choice: Equatable {
        /// No server, which is the default and not a failure. See [`choice`].
        case none
        case server(ServerAddress)
    }

    /// One key, and what its absence means.
    ///
    /// * **Absent** — nobody has said anything, and the answer is *this device on its
    ///   own*. A shopping list should open and be usable, not open and ask a question
    ///   about hosting; somebody who has a server goes and says so in settings. The
    ///   development build is the exception, and only because a debug build exists to
    ///   talk to the server on the same desk.
    /// * **A value** — what somebody entered.
    /// * **Empty** — the same as absent. It is what `forget` leaves behind, and it is
    ///   distinguishable from absent only in that the build setting no longer applies:
    ///   without it, "change server" on a build compiled with an address would clear
    ///   the stored one and fall straight back to the built-in, which is a button that
    ///   appears to work and does nothing.
    /// * **`local`** — somebody chose this deliberately. Stored rather than inferred
    ///   from absence so that the two can be told apart if they ever need to be.
    static var choice: Choice {
        // Nothing stored means nothing stored. This used to fall back to the build
        // setting, which made "the app opens straight into a usable list" true of a
        // release build and false of every build anybody actually runs: a fresh debug
        // install went to the machine on the desk and asked somebody to sign in.
        //
        // The build setting is not consulted at all. It was offered in settings as a
        // one-tap "use the machine on the desk", which is a developer's convenience on
        // a screen everybody else reads -- and a pre-filled address is a suggestion
        // about where somebody's shopping should live.
        guard let stored = UserDefaults.standard.string(forKey: key) else {
            return .none
        }

        if stored == onDeviceOnly || stored.isEmpty { return .none }

        guard case .success(let address) = ServerAddress.parse(stored)
        else { return .none }

        return .server(address)
    }

    /// The reserved value. Not a hostname anybody could type: `ServerAddress` refuses
    /// it for having no dot and no scheme it recognises... which is not true, so it is
    /// checked before parsing rather than relying on that.
    private static let onDeviceOnly = "local"

    static var current: ServerAddress? {
        if case .server(let address) = choice { return address }
        return nil
    }

    /// Records that this device is on its own.
    ///
    /// The app then works exactly as it does with no signal, which is not a
    /// coincidence: everything queues to the outbox and shows from the cache, and
    /// attaching a server later drains the queue into it. "No server" and "no signal"
    /// are the same state, and the app already knew how to be in one of them.
    static func onlyThisDevice() {
        UserDefaults.standard.set(onDeviceOnly, forKey: key)
        Log.info(.app, "this device is on its own now")
        announce()
    }

    /// Says the answer changed, because it is storage and nothing observes storage.
    private static func announce() {
        NotificationCenter.default.post(name: .serverChanged, object: nil)
    }

    /// What the build was pointed at, if anything.
    ///
    /// Read by nothing on a screen any more -- see `choice`. It survives because a
    /// debug build still has to reach the machine on the desk somehow, and this is
    /// where that address is written down.
    private static var built: ServerAddress? {
        guard
            let raw = Bundle.main.object(forInfoDictionaryKey: "ShoppingListAPIBaseURL")
                as? String,
            case .success(let address) = ServerAddress.parse(raw)
        else { return nil }

        return address
    }

    /// Whether this device is on its own, which is the default.
    ///
    /// There is deliberately no "has not been asked": nothing asks. A shopping list
    /// opens and is usable, and a server is something somebody goes and configures.
    static var isOnDeviceOnly: Bool { choice == .none }

    /// Records an address that has been checked, and says whether it is a *different*
    /// server from the one before — which is the caller's cue to throw everything local
    /// away, for the reason [`forget`] gives.
    @discardableResult
    static func remember(_ address: ServerAddress) -> Bool {
        let changed = current != address
        UserDefaults.standard.set(address.origin, forKey: key)
        // The address is somebody's hostname, so it is not written down here. Whether it
        // is a *different* one is the fact that matters, because it is what tells the
        // caller to throw everything local away.
        Log.info(.app, "a server was chosen", Detail("different", .flag(changed)))
        announce()
        return changed
    }

    /// Forgets the stored address, so the next launch asks again.
    ///
    /// **Callers must also clear the cache and sign out.** Not a precaution: the caches
    /// hold rows keyed by ids and uuids minted by the old server, and history and
    /// suggestions belong to an account on it. Carrying them across would show one
    /// server's lists under another server's name.
    static func forget() {
        // Emptied rather than removed: removing it would let a build compiled with an
        // address answer with that one — see `choice`.
        UserDefaults.standard.set("", forKey: key)
        Log.info(.app, "the server was given up")
        announce()
    }

    // MARK: - Asking

    /// What a server says about itself. The other end is `GET /api/server`.
    struct About: Decodable, Equatable {
        let name: String
        let version: String
        /// `open`, `closed` or `unclaimed`.
        let admission: String

        /// Nobody owns this server yet, so the first person to arrive claims it with
        /// the code from its log rather than signing in.
        var isUnclaimed: Bool { admission == "unclaimed" }

        /// Whether a stranger will be let in, so a sign-in screen can stop promising a
        /// refusal that will not come.
        var admitsAnyone: Bool { admission == "open" }
    }

    /// Why an address was not accepted, in the words the screen says.
    ///
    /// Three failures a person fixes in three different ways, so they get three
    /// sentences rather than "could not connect".
    enum Refusal: Error, Equatable {
        case unreachable
        case notThisSoftware
        case certificateRefused

        var sentence: String {
            switch self {
            case .unreachable:
                "Cannot reach that address. Check it, and check you are on the same network as the server."
            case .notThisSoftware:
                "Something is running there, but it is not a Shopping List server."
            case .certificateRefused:
                "That server's certificate could not be verified."
            }
        }
    }

    /// The name the server answers with. A mismatch is refused.
    static let software = "shopping-list"

    /// Asks an address whether it is a Shopping List server.
    ///
    /// C2: a regular expression proves the string is a URL. It does not prove there is a
    /// server there, that it is *this* server, or that TLS will negotiate — and all three
    /// fail in ways a person can fix. `GET /healthz` would not do either, since every
    /// health endpoint on the internet returns `ok`: pointing the app at an unrelated
    /// service would succeed and then fail confusingly at the first real call.
    static func ask(
        _ address: ServerAddress,
        using session: URLSession = .shared
    ) async -> Result<About, Refusal> {
        var request = URLRequest(url: address.url.appendingPathComponent("api/server"))
        request.timeoutInterval = 10

        do {
            let (data, response) = try await session.data(for: request)

            guard (response as? HTTPURLResponse)?.statusCode == 200,
                  let about = try? JSONDecoder().decode(About.self, from: data),
                  about.name == software
            else {
                return .failure(.notThisSoftware)
            }

            return .success(about)
        } catch let error as URLError where isCertificate(error.code) {
            return .failure(.certificateRefused)
        } catch {
            return .failure(.unreachable)
        }
    }

    /// Told apart from an ordinary failure because it is fixed differently: a
    /// certificate is something the operator repairs on the server, and an unreachable
    /// address is usually a typo or the wrong network.
    private static func isCertificate(_ code: URLError.Code) -> Bool {
        [
            .serverCertificateUntrusted,
            .serverCertificateHasBadDate,
            .serverCertificateHasUnknownRoot,
            .serverCertificateNotYetValid,
            .secureConnectionFailed,
        ].contains(code)
    }
}

extension Notification.Name {
    /// Which server this device uses has changed.
    ///
    /// `ServerDirectory` is `UserDefaults`, and nothing observes `UserDefaults`. The
    /// screens that branch on it are told rather than left to notice, which is the
    /// difference between the app changing when somebody changes it and the app
    /// changing next time it is launched.
    static let serverChanged = Notification.Name("shoppinglist.serverChanged")
}
