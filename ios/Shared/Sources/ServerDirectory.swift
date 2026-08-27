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
    /// One key, three states, and the third is the one that is easy to miss.
    ///
    /// * **Absent** — nobody has ever been asked, so the build setting applies.
    /// * **A value** — what somebody entered.
    /// * **Empty** — somebody deliberately cleared it, and the build setting must
    ///   *not* apply. Without this state, "change server" on a build compiled with an
    ///   address would clear the stored one and fall straight back to the built-in,
    ///   which is a button that appears to work and does nothing.
    static var current: ServerAddress? {
        guard let stored = UserDefaults.standard.string(forKey: key) else { return built }

        guard case .success(let address) = ServerAddress.parse(stored, allowingCleartext: true)
        else { return nil }

        return address
    }

    /// What the build was pointed at, if anything.
    ///
    /// Cleartext is allowed here whatever the build says: this value came from somebody
    /// compiling the app rather than from a text field, and refusing it would only break
    /// a simulator talking to a server on the same desk.
    private static var built: ServerAddress? {
        guard
            let raw = Bundle.main.object(forInfoDictionaryKey: "ShoppingListAPIBaseURL")
                as? String,
            case .success(let address) = ServerAddress.parse(raw, allowingCleartext: true)
        else { return nil }

        return address
    }

    /// Whether anybody has to be asked. False on a development build, which has one.
    static var needsAnAddress: Bool { current == nil }

    /// Records an address that has been checked, and says whether it is a *different*
    /// server from the one before — which is the caller's cue to throw everything local
    /// away, for the reason [`forget`] gives.
    @discardableResult
    static func remember(_ address: ServerAddress) -> Bool {
        let changed = current != address
        UserDefaults.standard.set(address.origin, forKey: key)
        return changed
    }

    /// Forgets the stored address, so the next launch asks again.
    ///
    /// **Callers must also clear the cache and sign out.** Not a precaution: the caches
    /// hold rows keyed by ids and uuids minted by the old server, and history and
    /// suggestions belong to an account on it. Carrying them across would show one
    /// server's lists under another server's name.
    static func forget() {
        // Emptied rather than removed: removing it would mean "never asked", and a
        // build with a compiled-in address would answer with that one — see `current`.
        UserDefaults.standard.set("", forKey: key)
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
