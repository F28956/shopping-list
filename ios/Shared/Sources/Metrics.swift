import Foundation

#if os(iOS)
    import UIKit
#endif

/// Where measurements go, if anywhere.
///
/// Beside the server address in Settings, because it is the same kind of decision made
/// by the same kind of person: somebody who runs their own things and wants to see how
/// they are doing. It defaults to nowhere, and nowhere is a complete answer.
enum MetricsSettings {
    private static let endpointKey = "metrics.endpoint"
    private static let headersKey = "metrics.headers"
    private static let instanceKey = "metrics.instance"

    /// The collector's metrics URL — the full one, ending in `/v1/metrics`.
    ///
    /// Stored as typed rather than as a host with a path appended here: collectors sit
    /// behind gateways, on odd ports, under prefixes, and guessing at the path is how
    /// somebody ends up with a 404 they cannot see and a setting that looks fine.
    static var endpoint: URL? {
        get {
            guard let raw = UserDefaults.standard.string(forKey: endpointKey),
                  !raw.isEmpty,
                  let url = URL(string: raw),
                  url.scheme == "https" || url.scheme == "http"
            else { return nil }
            return url
        }
        set {
            UserDefaults.standard.set(newValue?.absoluteString ?? "", forKey: endpointKey)
            NotificationCenter.default.post(name: .diagnosticsChanged, object: nil)
        }
    }

    /// What somebody typed, kept as typed so the settings field can show it back.
    ///
    /// One `Name: value` per line. Parsed on the way out rather than on the way in, so a
    /// half-typed header is a header that does nothing rather than a field that refuses
    /// to hold what is being typed into it.
    static var rawHeaders: String {
        get { UserDefaults.standard.string(forKey: headersKey) ?? "" }
        set {
            UserDefaults.standard.set(newValue, forKey: headersKey)
            NotificationCenter.default.post(name: .diagnosticsChanged, object: nil)
        }
    }

    static var headers: [String: String] {
        var found: [String: String] = [:]
        for line in rawHeaders.split(whereSeparator: \.isNewline) {
            let parts = line.split(separator: ":", maxSplits: 1)
            guard parts.count == 2 else { continue }
            let name = parts[0].trimmingCharacters(in: .whitespaces)
            let value = parts[1].trimmingCharacters(in: .whitespaces)
            guard !name.isEmpty, !value.isEmpty else { continue }
            found[name] = value
        }
        return found
    }

    /// Whether anything is measured at all.
    ///
    /// **Two conditions, and the first is not negotiable.** A device answering for
    /// itself reports nothing, ever — `Capabilities.syncing` is the same question asked
    /// of the screens, and it is false exactly when there is no far end. Somebody who
    /// chose to keep their shopping on one phone did not choose to have that phone
    /// describe itself to a third party, however anonymously, and an app whose whole
    /// premise is "your lists do not reach me" cannot have an exception in it. The
    /// second is that somebody has entered a collector.
    ///
    /// **Always false on the watch**, and for a reason that is about credentials rather
    /// than effort. A collector wants an endpoint and usually a header to authenticate
    /// with. The endpoint could reach the wrist the way the server's address does — in
    /// the application context — but the header cannot: a context is persisted and
    /// latest-wins, which is exactly why `WatchLink` refuses to put a session token in
    /// one. A watch reporting metrics would need a second credential channel, built for
    /// telemetry, on the device with the least to say. So the watch logs and does not
    /// measure, and what is worth knowing about the link (snapshots pushed, ticks
    /// replayed) is measured from the phone's end, where both are visible anyway.
    ///
    /// The calls are compiled on the watch all the same rather than fenced off with
    /// `#if`: `Store/Sources` and `Shared/Sources` are the same source in all three apps,
    /// and a platform check around every call site is how one of them comes to be
    /// written differently from the others.
    static var reporting: Bool {
        #if os(watchOS)
            return false
        #else
            return Capabilities.current.syncing && endpoint != nil
        #endif
    }

    /// A name for this installation, minted here and never sent anywhere else.
    ///
    /// OpenTelemetry wants a `service.instance.id` so that two devices reporting the
    /// same series can be told apart; without one, a phone and a Mac average into a
    /// meaningless middle. A random uuid rather than anything the system knows about
    /// this device: an identifier for advertising, a vendor id or a device name would
    /// each be a fact about somebody, and this is a fact about an install.
    static var instance: String {
        if let stored = UserDefaults.standard.string(forKey: instanceKey) { return stored }
        let minted = UUID().uuidString
        UserDefaults.standard.set(minted, forKey: instanceKey)
        return minted
    }
}

/// One attribute on a measurement.
///
/// The same ``Plain`` the log's `info` lines take, and that is the point: **a metric
/// name, label or attribute may not carry personal data either**, and rather than
/// writing the rule down twice it is the same type enforcing it. There is no way to put
/// a list name into a series here for the same reason there is no way to put one into an
/// `info` line — the value cannot be a `String` and the name can only be a literal.
struct Tagged: Hashable, Sendable {
    let name: String
    let value: String

    init(_ name: StaticString, _ value: Plain) {
        self.name = String(describing: name)
        self.value = value.written
    }
}

/// What this app measures about itself, and where it sends it.
///
/// ## Why this is written out rather than taken as a dependency
///
/// The OpenTelemetry Swift SDK is a package graph — the API, the SDK, an exporter,
/// swift-protobuf and gRPC underneath it — and what is needed here is one POST of one
/// JSON body, on a timer, from three app targets that already share every line of their
/// plumbing. The SDK earns its size in a service with traces, spans, context
/// propagation and a dozen instrumented libraries; this app has fourteen counters and
/// two histograms and no traces at all. It would also be the first dependency in these
/// targets that is not GRDB, on a project whose stated promise is that nothing about
/// somebody's shopping leaves the machine they chose — and a telemetry SDK is precisely
/// the dependency where that promise wants the fewest moving parts nobody here has read.
///
/// The cost of writing it out is this file: the OTLP/HTTP JSON shape, delta temporality,
/// and a fixed set of histogram buckets. That shape is a stable, published protocol; if
/// it ever needs traces, the SDK is the right answer and this is a hundred and fifty
/// lines to delete.
///
/// ## What it collects
///
/// Nothing, until somebody has both a server and a collector — see
/// ``MetricsSettings/reporting``. Then: how long requests take and how they end, how
/// deep the queue is and what draining it achieved, whether the change stream is up,
/// when the app decides it is offline, what sync refused, and that the app launched.
final class Metrics: @unchecked Sendable {

    static let shared = Metrics()

    /// How often what has been collected is sent.
    ///
    /// A minute. Short enough that a phone put down mid-shop has reported what happened,
    /// long enough that the report is one request rather than a stream of them — this
    /// runs on a battery, and a radio woken every five seconds costs more than the thing
    /// being measured.
    static let interval: TimeInterval = 60

    private let lock = NSLock()
    private var counters: [Instrument: Int] = [:]
    private var gauges: [Instrument: Int] = [:]
    private var histograms: [Instrument: Histogram] = [:]
    private var since = Date()
    private var sending: Task<Void, Never>?

    /// A metric name with its attributes, which together name one series.
    struct Instrument: Hashable, Sendable {
        let name: String
        let attributes: [Tagged]
    }

    /// Where a duration falls, in milliseconds.
    ///
    /// Fixed bounds rather than exponential ones: the questions worth asking of this app
    /// are "did it feel instant", "did it feel slow" and "did it time out", and these
    /// bracket all three. Chosen once and left alone — changing bucket bounds rewrites
    /// history in every dashboard drawn from them.
    static let bounds: [Double] = [5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000]

    struct Histogram: Sendable {
        var count = 0
        var sum: Double = 0
        var buckets = [Int](repeating: 0, count: Metrics.bounds.count + 1)

        mutating func add(_ value: Double) {
            count += 1
            sum += value
            let bucket = Metrics.bounds.firstIndex { value <= $0 } ?? Metrics.bounds.count
            buckets[bucket] += 1
        }
    }

    // MARK: - Recording

    /// One more of something.
    func count(_ name: StaticString, _ attributes: Tagged..., by amount: Int = 1) {
        guard MetricsSettings.reporting else { return }
        let key = Instrument(name: String(describing: name), attributes: attributes)
        lock.withLock { counters[key, default: 0] += amount }
    }

    /// What something is *now*, replacing whatever it was.
    ///
    /// For depths and sizes, where the sum of the readings means nothing and the last
    /// one means everything. Queue depth is the example: adding up every reading of it
    /// would say a queue of one that never drained was a queue of six hundred.
    func gauge(_ name: StaticString, _ value: Int, _ attributes: Tagged...) {
        guard MetricsSettings.reporting else { return }
        let key = Instrument(name: String(describing: name), attributes: attributes)
        lock.withLock { gauges[key] = value }
    }

    /// How long something took.
    func observe(_ name: StaticString, milliseconds: Double, _ attributes: Tagged...) {
        guard MetricsSettings.reporting else { return }
        let key = Instrument(name: String(describing: name), attributes: attributes)
        lock.withLock { histograms[key, default: Histogram()].add(milliseconds) }
    }

    // MARK: - Sending

    /// Starts the timer, if there is anywhere to send to.
    ///
    /// Called at every composition root and safe to call again: choosing a collector in
    /// Settings calls it, and so does giving one up, which is what stops it.
    func start() {
        sending?.cancel()
        sending = nil
        guard MetricsSettings.reporting else {
            // Whatever was collected before somebody turned this off is thrown away
            // rather than held for a collector that may never be named again.
            lock.withLock {
                counters.removeAll()
                gauges.removeAll()
                histograms.removeAll()
            }
            return
        }

        sending = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(Metrics.interval))
                guard !Task.isCancelled else { return }
                await self?.flush()
            }
        }

        #if os(iOS)
            // A phone put in a pocket is a phone that may not come back before the
            // system suspends it, and a suspended app's timer does not fire. The last
            // minute of a shopping trip is the minute worth having.
            NotificationCenter.default.addObserver(
                forName: UIApplication.willResignActiveNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { await self?.flush() }
            }
        #endif
    }

    /// Sends what has been collected, and forgets it.
    ///
    /// Delta rather than cumulative, which is the one real decision in this file.
    /// Cumulative means holding every series for the life of the process and restarting
    /// the counts on relaunch — a collector then has to guess where a reset happened,
    /// and an app that is launched and killed twenty times a day gives it twenty chances
    /// to guess wrong. Delta means each report stands alone, so a report that is dropped
    /// costs that minute and nothing after it.
    func flush() async {
        guard MetricsSettings.reporting, let endpoint = MetricsSettings.endpoint else { return }

        let taken: (counters: [Instrument: Int], gauges: [Instrument: Int], histograms: [Instrument: Histogram], from: Date) =
            lock.withLock {
                defer {
                    counters.removeAll()
                    histograms.removeAll()
                    since = Date()
                    // Gauges are *not* cleared: a depth of nought is a fact worth
                    // reporting every minute, and a series that stops arriving is
                    // indistinguishable from a device that has been switched off.
                }
                return (counters, gauges, histograms, since)
            }

        guard !taken.counters.isEmpty || !taken.gauges.isEmpty || !taken.histograms.isEmpty else {
            return
        }

        let body = OTLP.body(
            counters: taken.counters,
            gauges: taken.gauges,
            histograms: taken.histograms,
            from: taken.from,
            to: Date()
        )
        guard let encoded = try? JSONSerialization.data(withJSONObject: body) else { return }

        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        // Whatever the collector needs to believe this. Never logged: the whole reason
        // this field exists is that it holds a credential.
        for (name, value) in MetricsSettings.headers {
            request.setValue(value, forHTTPHeaderField: name)
        }
        request.httpBody = encoded
        request.timeoutInterval = 15

        do {
            let (_, response) = try await URLSession.shared.data(for: request)
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            if !(200..<300).contains(status) {
                // Warned rather than retried. A collector refusing a report is a
                // configuration mistake somebody has to go and fix, and a client that
                // retries into it turns one wrong header into a request every minute for
                // as long as the app is installed.
                Log.warn(
                    .metrics, "the collector refused a report",
                    Detail("status", .count(status)),
                    Detail("series", .count(taken.counters.count + taken.gauges.count + taken.histograms.count))
                )
            }
        } catch {
            // Dropped. Measurements about a shop with no signal are the least important
            // thing in the queue behind them, and holding them would mean a second
            // outbox with none of the first one's care.
            Log.warn(
                .metrics, "could not report",
                Detail("why", .failure(Plain.Failure(error)))
            )
        }
    }
}

/// The names, in one place.
///
/// Written out as constants so that a name is chosen once rather than typed at each call
/// site, where a `queue.drained` and a `queue.drain` would become two series that each
/// tell half the story. Dotted and lowercase, which is what the OpenTelemetry naming
/// conventions ask for.
enum Measured {
    /// How long a request to the server took, by route class and outcome.
    static let requestDuration: StaticString = "shoppinglist.request.duration"
    /// How many requests, by route class and outcome. Derivable from the histogram's
    /// count, and kept separately because a failure that never returned has a duration
    /// nobody should read as a latency.
    static let requests: StaticString = "shoppinglist.request.count"
    /// How many changes are waiting to be sent, right now.
    static let queueDepth: StaticString = "shoppinglist.queue.depth"
    /// What a drain achieved, by result.
    static let queueDrained: StaticString = "shoppinglist.queue.drained"
    /// A change stream opening, dropping, or refusing to open.
    static let streamState: StaticString = "shoppinglist.stream.state"
    /// A nudge arriving on one, by kind.
    static let streamNudge: StaticString = "shoppinglist.stream.nudge"
    /// The app deciding the far end is or is not there.
    static let reachability: StaticString = "shoppinglist.reachability.changed"
    /// Something the server would not accept, or a tick against a row that had gone.
    static let syncRefused: StaticString = "shoppinglist.sync.refused"
    static let syncLost: StaticString = "shoppinglist.sync.lost"
    /// A snapshot handed to the watch, and what became of it.
    static let watchSnapshot: StaticString = "shoppinglist.watch.snapshot"
    /// A cache or database write that failed.
    static let storeFailed: StaticString = "shoppinglist.store.failed"
    /// A migration or a handover between this device's store and a server's.
    static let handover: StaticString = "shoppinglist.handover"
    /// The app starting, by mode.
    static let launch: StaticString = "shoppinglist.launch"
}

