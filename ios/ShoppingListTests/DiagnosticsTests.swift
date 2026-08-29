import Foundation
import Testing

@testable import ShoppingList

/// What the log is allowed to say, and when it says anything at all.
///
/// The rule these are about: `info`, `warn` and `error` must never carry personal data,
/// and `trace` and `debug` may carry anything but are off until somebody turns them on.
/// Both halves are enforced by types rather than by convention, and what a test can add
/// to that is the part the compiler cannot check — that the *rendering* of a permitted
/// value cannot reproduce what somebody typed, and that the level actually gates.
struct LoggingTests {

    /// A book writing into a directory of its own, so two of these running at once do
    /// not read each other's lines.
    private func book(at level: LogLevel) -> (LogBook, LogFile) {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("logtest-\(UUID().uuidString)", isDirectory: true)
        let file = LogFile(in: folder, named: "test")
        return (LogBook(file: file, level: level, subsystem: "shoppinglist.tests"), file)
    }

    private func written(_ file: LogFile) -> String {
        file.settled()
            .compactMap { try? String(contentsOf: $0, encoding: .utf8) }
            .joined()
    }

    // MARK: - The rule

    /// **The one that matters.**
    ///
    /// An item name reaches a log line at `info` in exactly one way: somebody puts it
    /// there. This proves that the way is closed — the message is a `StaticString`, so it
    /// is whatever was typed in the source, and every value beside it is a ``Plain``,
    /// whose cases are counts, ids, durations and words from closed vocabularies.
    ///
    /// The compiler enforces the other half, and deliberately cannot be tested from
    /// here: `Log.info(.backend, "added", Detail("name", .word(item.name)))` does not
    /// compile, because `.word` takes a `StaticString` and a `String` is not one, and
    /// neither is there a case that would take it. A test that demonstrated that would
    /// have to fail to build.
    @Test("nothing said at info can spell an item name")
    func infoCannotCarryAnItemName() {
        let named = "pregnancy test"
        let list = "Boots run"

        let (book, file) = book(at: .info)
        book.info(
            .backend, "added a row",
            Detail("list", .id(41)),
            Detail("items", .count(7)),
            Detail("took", .milliseconds(120)),
            Detail("offline", .flag(true)),
            Detail("route", .route(.listItems)),
            Detail("outcome", .request(.ok)),
            Detail("why", .failure(.transport)),
            Detail("kind", .word("add"))
        )
        book.warn(.queue, "the queue did not move", Detail("waiting", .count(3)))
        book.error(.store, "a statement failed", Detail("doing", .word("write")))

        let text = written(file)
        #expect(!text.isEmpty)
        #expect(!text.contains(named))
        #expect(!text.contains(list))
    }

    /// Every case of ``Plain``, rendered, against a shopping list that would be
    /// mortifying to leak.
    ///
    /// A belt to the type system's braces: the cases are closed today, and this is what
    /// fails the day somebody adds `case text(String)` to the enum and starts using it.
    @Test("no value a Plain can hold renders as free text")
    func plainRendersOnlyClosedVocabularies() {
        let rendered: [String] = [
            Plain.count(3), .id(-17), .milliseconds(84), .flag(false),
            .route(.suggestions), .outcome(.refused), .request(.unreachable),
            .failure(.notAdmitted), .word("handover"),
        ].map(\.written)

        // Everything a value may render as, spelled out. A new case has to be added here
        // deliberately, which is the moment to ask whether it can hold somebody's data.
        let allowed = Set(
            Route.allCases.map(\.rawValue)
                + ["ok", "refused", "lost", "waiting", "unreachable", "cancelled", "skipped"]
                + ["rejected", "failed"]
                + ["transport", "unauthorized", "forbidden", "notAdmitted", "notFound",
                   "badInput", "server", "decoding", "database", "other"]
                + ["true", "false", "handover"]
        )

        for value in rendered {
            // A number, a duration, or a word from a vocabulary. Nothing else.
            let isNumber = Int(value) != nil
            let isDuration = value.hasSuffix("ms") && Int(value.dropLast(2)) != nil
            #expect(isNumber || isDuration || allowed.contains(value), "\(value) is free text")
        }
    }

    /// `debug` and `trace` are the levels that *may* say it, which is the other half of
    /// the rule and the reason the setting shows a warning first.
    @Test("debug may carry the contents, and does")
    func debugMayCarryContents() {
        let named = "pregnancy test"
        let (book, file) = book(at: .debug)
        book.debug(.backend, "added \(named)")

        #expect(written(file).contains(named))
        #expect(LogLevel.debug.mayCarryContents)
        #expect(LogLevel.trace.mayCarryContents)
        #expect(!LogLevel.info.mayCarryContents)
        #expect(!LogLevel.warn.mayCarryContents)
        #expect(!LogLevel.error.mayCarryContents)
    }

    // MARK: - The switch

    /// Off is `warn`, and off is the default.
    ///
    /// Not silence: a warning and an error are written whether or not anybody asked,
    /// because the alternative is an app that says nothing about a failure until
    /// somebody reproduces it with the switch on -- which is the failure nobody can
    /// reproduce.
    @Test("with logging off, only warnings and errors are written")
    func offKeepsFailures() {
        let (book, file) = book(at: .warn)
        book.trace(.app, "traced")
        book.debug(.app, "debugged")
        book.info(.app, "informed")
        book.warn(.app, "warned")
        book.error(.app, "failed")

        let text = written(file)
        #expect(!text.contains("traced"))
        #expect(!text.contains("debugged"))
        #expect(!text.contains("informed"))
        #expect(text.contains("warned"))
        #expect(text.contains("failed"))
    }

    @Test("turning it up lets the quieter levels through")
    func turningItUp() {
        let (book, file) = book(at: .warn)
        book.info(.app, "before")
        book.level = .trace
        book.trace(.app, "after")
        book.info(.app, "also after")

        let text = written(file)
        #expect(!text.contains("before"))
        #expect(text.contains("after"))
        #expect(text.contains("also after"))
    }

    @Test("turning it back down stops it again")
    func turningItDown() {
        let (book, file) = book(at: .trace)
        book.trace(.app, "while on")
        book.level = .warn
        book.trace(.app, "while off")
        book.debug(.app, "also while off")

        let text = written(file)
        #expect(text.contains("while on"))
        #expect(!text.contains("while off"))
        #expect(!text.contains("also while off"))
    }

    @Test("a level survives being written down and read back")
    func levelsRoundTrip() {
        for level in LogLevel.allCases {
            #expect(LogLevel(stored: level.stored) == level)
        }
        // A word this build does not know is the default, not a crash and not tracing.
        #expect(LogLevel(stored: "verbose") == nil)
    }

    // MARK: - The file

    /// The cap is what stops a log from filling a phone. Rolled before the write rather
    /// than after, so a file never exceeds it even by one line.
    @Test("the file rolls rather than growing")
    func theFileRolls() {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("logtest-\(UUID().uuidString)", isDirectory: true)
        let file = LogFile(in: folder, named: "test", limit: 512)
        let book = LogBook(file: file, level: .trace, subsystem: "shoppinglist.tests")

        for i in 0..<200 { book.trace(.app, "line number \(i)") }

        let kept = file.settled()
        #expect(kept.count == 2)
        for url in kept {
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size]) as? Int
            #expect((size ?? 0) <= 512)
        }
        // The end of the run survived, which is the end anybody reads.
        #expect(written(file).contains("line number 199"))
    }

    /// What the watch ships to the phone: the tail, capped, and starting at a line
    /// boundary rather than halfway through a timestamp.
    @Test("the tail is bounded and begins on a line")
    func theTailIsBounded() {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("logtest-\(UUID().uuidString)", isDirectory: true)
        let file = LogFile(in: folder, named: "watch")
        let book = LogBook(file: file, level: .trace, subsystem: "shoppinglist.tests")
        for i in 0..<500 { book.trace(.watch, "a line of some length, number \(i)") }

        let tail = file.tail(bytes: 2000)
        #expect(tail.count <= 2000)
        let text = String(decoding: tail, as: UTF8.self)
        #expect(text.hasPrefix("2"))  // a timestamp, so the cut was on a line boundary
        #expect(text.contains("number 499"))
    }
}

/// Folding a path down to a class, which is what a log line and a metric label are
/// allowed to say about which request this was.
struct RouteTests {

    @Test("a path becomes a class and loses its numbers")
    func pathsFold() {
        #expect(Route(path: "/api/lists") == .lists)
        #expect(Route(path: "/api/lists/41") == .lists)
        #expect(Route(path: "/api/lists/41/items") == .listItems)
        #expect(Route(path: "/api/lists/41/items/9/done") == .listItems)
        #expect(Route(path: "/api/lists/41/items/done") == .listItems)
        #expect(Route(path: "/api/lists/41/items/9/tags/3") == .itemTags)
        #expect(Route(path: "/api/lists/41/tag-order") == .listTags)
        #expect(Route(path: "/api/lists/41/events") == .listEvents)
        #expect(Route(path: "/api/lists/41/members") == .listPeople)
        #expect(Route(path: "/api/lists/41/members/invites") == .listInvites)
        #expect(Route(path: "/api/lists/41/history/entries") == .history)
        #expect(Route(path: "/api/me/events") == .myEvents)
        #expect(Route(path: "/api/sync") == .sync)
        #expect(Route(path: "/api/units") == .units)
        #expect(Route(path: "/api/tags/7") == .tags)
    }

    /// The reason this type exists. A query string carries what somebody is typing into
    /// the add field, and one path carries an email address.
    @Test("neither a query string nor an id survives")
    func nothingOfTheRequestSurvives() {
        let folded = Route(path: "/api/lists/41/history?q=pregnancy%20test&size=20")
        #expect(folded == .suggestions)
        #expect(!folded.rawValue.contains("pregnancy"))
        #expect(!folded.rawValue.contains("41"))
    }

    /// The one route with a person's address in the URL.
    @Test("an email in a path does not become a label")
    func anEmailDoesNotSurvive() {
        let folded = Route(path: "/api/admissions/someone%40example.com/owner")
        #expect(folded == .admissions)
        #expect(!folded.rawValue.contains("example"))
        #expect(!folded.rawValue.contains("someone"))
    }

    /// A route nobody has classified is `other` rather than passed through, which is the
    /// difference between a new endpoint being uninteresting and one quietly leaking its
    /// arguments into a label.
    @Test("an unknown path is other, not itself")
    func unknownIsOther() {
        #expect(Route(path: "/api/lists/41/secrets/abc123") == .other)
        #expect(Route(path: "/whatever") == .other)
    }
}

/// What is measured, and what may be said about it.
struct MetricsTests {

    /// The same rule as the log's, enforced by the same type: an attribute's value is a
    /// ``Plain`` and its name is a literal, so there is no series here that can be keyed
    /// by a list name.
    @Test("an attribute cannot be spelled from somebody's data")
    func attributesAreClosed() {
        let tagged = Tagged("route", .route(.listItems))
        #expect(tagged.name == "route")
        #expect(tagged.value == "listItems")
    }

    @Test("headers are read a line at a time, and half-typed ones do nothing")
    func headersParse() {
        let before = MetricsSettings.rawHeaders
        defer { MetricsSettings.rawHeaders = before }

        MetricsSettings.rawHeaders = """
            Authorization: Bearer abc123
            X-Scope-OrgID:  household
            this line is not a header
            """
        let found = MetricsSettings.headers
        #expect(found["Authorization"] == "Bearer abc123")
        #expect(found["X-Scope-OrgID"] == "household")
        #expect(found.count == 2)
    }

    /// A collector that is not an address is no collector, and the field says so rather
    /// than looking as though it were filled in.
    @Test("only an http address is a collector")
    func endpointsAreChecked() {
        let before = MetricsSettings.endpoint
        defer { MetricsSettings.endpoint = before }

        MetricsSettings.endpoint = URL(string: "ftp://example.com/v1/metrics")
        #expect(MetricsSettings.endpoint == nil)

        MetricsSettings.endpoint = URL(string: "https://collector.example.com/v1/metrics")
        #expect(MetricsSettings.endpoint?.host == "collector.example.com")

        MetricsSettings.endpoint = nil
        #expect(MetricsSettings.endpoint == nil)
        // Which is also the whole of "nothing is reported": no collector, no report.
        #expect(!MetricsSettings.reporting)
    }

    /// The trap OTLP/JSON sets. Protobuf's JSON mapping writes 64-bit integers as
    /// strings, and a collector handed a number for `timeUnixNano` rejects the report
    /// with an error nobody here would ever see.
    @Test("every 64-bit number in the payload is a string")
    func sixtyFourBitNumbersAreStrings() {
        let key = Metrics.Instrument(
            name: "shoppinglist.request.count",
            attributes: [Tagged("route", .route(.lists)), Tagged("outcome", .request(.ok))]
        )
        var histogram = Metrics.Histogram()
        histogram.add(42)

        let body = OTLP.body(
            counters: [key: 3],
            gauges: [Metrics.Instrument(name: "shoppinglist.queue.depth", attributes: []): 5],
            histograms: [Metrics.Instrument(name: "shoppinglist.request.duration", attributes: []): histogram],
            from: Date(timeIntervalSince1970: 1_700_000_000),
            to: Date(timeIntervalSince1970: 1_700_000_060)
        )

        let metrics = ((body["resourceMetrics"] as? [[String: Any]])?
            .first?["scopeMetrics"] as? [[String: Any]])?
            .first?["metrics"] as? [[String: Any]]
        #expect(metrics?.count == 3)

        for metric in metrics ?? [] {
            let points = (metric["sum"] as? [String: Any] ?? metric["gauge"] as? [String: Any]
                ?? metric["histogram"] as? [String: Any])?["dataPoints"] as? [[String: Any]]
            for point in points ?? [] {
                #expect(point["timeUnixNano"] is String)
                #expect(point["startTimeUnixNano"] is String)
                if let asInt = point["asInt"] { #expect(asInt is String) }
                if let count = point["count"] { #expect(count is String) }
                if let buckets = point["bucketCounts"] { #expect(buckets is [String]) }
            }
        }

        // And the whole thing is something `JSONSerialization` will actually write.
        #expect((try? JSONSerialization.data(withJSONObject: body)) != nil)
    }

    /// The names on the wire, which must not carry anything either. Checked as a set
    /// rather than one at a time so that adding a metric means looking at this list.
    @Test("no metric is named after anything of somebody's")
    func namesAreOurs() {
        let names = [
            Measured.requestDuration, Measured.requests, Measured.queueDepth,
            Measured.queueDrained, Measured.streamState, Measured.streamNudge,
            Measured.reachability, Measured.syncRefused, Measured.syncLost,
            Measured.watchSnapshot, Measured.storeFailed, Measured.handover,
            Measured.launch,
        ].map { String(describing: $0) }

        for name in names {
            #expect(name.hasPrefix("shoppinglist."))
            #expect(name.lowercased() == name)
        }
        #expect(Set(names).count == names.count)
    }
}
