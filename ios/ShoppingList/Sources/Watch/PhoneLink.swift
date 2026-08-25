import Foundation
import WatchConnectivity

/// The phone's half of the link to the watch.
///
/// The watch cannot sign in: Google's SDK has no watchOS build, and a watch has no
/// browser to run the flow in. So the phone, which does hold the credential, hands
/// over a current ID token when asked.
///
/// Asked, rather than pushed on a timer. A Google ID token lasts about an hour, and a
/// token pushed when the phone felt like it is stale exactly when the watch needs it
/// — standing in a shop, an hour after the phone was last opened. A request-reply
/// costs a round trip and is never out of date.
final class PhoneLink: NSObject, WCSessionDelegate {
    private let token: () async -> String?

    init(token: @escaping () async -> String?) {
        self.token = token
        super.init()

        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()
    }

    func session(
        _ session: WCSession,
        activationDidCompleteWith state: WCSessionActivationState,
        error: Error?
    ) {}

    // Required on iOS, and both are ordinary: switching watches, or taking one off.
    func sessionDidBecomeInactive(_ session: WCSession) {}

    func sessionDidDeactivate(_ session: WCSession) {
        // Re-activating is what picks up the newly paired watch.
        WCSession.default.activate()
    }

    func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any],
        replyHandler: @escaping ([String: Any]) -> Void
    ) {
        guard message[WatchLink.tokenRequest] != nil else {
            replyHandler([:])
            return
        }

        Task {
            // An empty reply rather than none: the watch is waiting, and a reply that
            // never comes leaves it spinning until WatchConnectivity times out.
            guard let token = await token() else {
                replyHandler([:])
                return
            }
            replyHandler([WatchLink.tokenRequest: token])
        }
    }
}
