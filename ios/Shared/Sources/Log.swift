import Foundation
import OSLog

/// How much this app is willing to write down about itself.
///
/// Ordered, and compared: a book set to ``warn`` keeps warnings and errors and drops
/// everything above them in this list.
enum LogLevel: Int, Comparable, CaseIterable, Sendable {
    case trace = 0
    case debug
    case info
    case warn
    case error

    static func < (a: LogLevel, b: LogLevel) -> Bool { a.rawValue < b.rawValue }

    /// What is stored, and what is read back.
    ///
    /// A word rather than the raw value: a number in a plist is a thing nobody can read
    /// and a thing that changes meaning the day a level is inserted in the middle.
    var stored: String {
        switch self {
        case .trace: "trace"
        case .debug: "debug"
        case .info: "info"
        case .warn: "warn"
        case .error: "error"
        }
    }

    init?(stored: String) {
        guard let found = Self.allCases.first(where: { $0.stored == stored }) else {
            return nil
        }
        self = found
    }

    /// Whether a log at this level may carry what is on somebody's list.
    ///
    /// The whole of the rule, in one place, so the settings screen and the type system
    /// agree about which levels the warning is shown for.
    var mayCarryContents: Bool { self <= .debug }

    /// The single word that starts a line in the file.
    var initial: String {
        switch self {
        case .trace: "TRACE"
        case .debug: "DEBUG"
        case .info: "INFO "
        case .warn: "WARN "
        case .error: "ERROR"
        }
    }

    /// Unified logging's nearest equivalent.
    ///
    /// `warn` is `.default` rather than `.error` on purpose: Console colours `.error`
    /// red and somebody scanning for a real failure should not have to read past a
    /// queue that did not drain because a phone is in a basement, which is the most
    /// common warning this app writes.
    var osLevel: OSLogType {
        switch self {
        case .trace, .debug: .debug
        case .info: .info
        case .warn: .default
        case .error: .error
        }
    }
}

/// Which part of the app is speaking.
///
/// A closed set rather than a free string, so that the categories in Console are the
/// same ones every release and a filter somebody saved keeps working.
enum LogArea: String, Sendable, CaseIterable {
    /// Reads and writes against whatever answers this app's questions.
    case backend
    /// The outbox and what became of it.
    case queue
    /// Change streams: connecting, dropping, and what arrives on them.
    case stream
    /// The link to the watch, from either end.
    case watch
    /// The cache and the device's own database.
    case store
    /// Migrations and handovers between the two of them.
    case handover
    /// Launching, and which mode the app came up in.
    case app
    /// The metrics exporter talking about itself.
    case metrics
}

// MARK: - What may be said at info and above

/// A value that is safe in a log line at `info`, `warn` or `error`.
///
/// The hard rule this exists for: **those three levels must never carry personal
/// data** — no item names, no list names, no email addresses, no tokens, no invite
/// codes. Counts, shapes, ids, durations and outcomes are fine.
///
/// It is a closed set of cases and not a string, which is what makes the rule
/// structural rather than a convention somebody has to remember at four in the
/// afternoon. There is no `case text(String)` and there must never be one: the moment
/// an arbitrary `String` can be put in a field, "no item names at info" goes back to
/// being a code-review habit, and code-review habits are how `add(_:to:)` came to log
/// the line somebody typed.
///
/// The message beside these fields is a ``StaticString``, which the compiler will only
/// build from a literal. So an `info` line is a constant sentence plus these values,
/// and there is no expression that puts somebody's shopping into either half.
enum Plain: Sendable {
    /// How many of something.
    case count(Int)
    /// A row, list or tag number. Ids name things; they do not spell them.
    case id(Int64)
    /// How long something took.
    case milliseconds(Int)
    case flag(Bool)
    /// A class of route — never the path, which would put ids and query strings in.
    case route(Route)
    /// How something ended, from a closed vocabulary.
    case outcome(Outcome)
    /// How a request came back — see ``RequestOutcome``, which is the vocabulary the
    /// metrics use, so a log line and a chart say the same word about the same thing.
    case request(RequestOutcome)
    /// What kind of failure this was, having been classified. Never the message: a
    /// server's `400` body says things like "there is already a list called that".
    case failure(Failure)
    /// A word from the source, for the cases the vocabularies above do not cover.
    ///
    /// `StaticString`, so it can only have been typed by whoever wrote the line. A
    /// `String` here would be the hole every other case is shaped to avoid.
    case word(StaticString)

    /// How something ended.
    enum Outcome: String, Sendable {
        case ok
        case refused
        case lost
        case waiting
        case unreachable
        case cancelled
        case skipped
    }

    /// What kind of failure, with nothing of the failure's own words in it.
    ///
    /// Built by ``init(_:)`` from an `Error`, which is the only way a failure reaches a
    /// log line: `error.localizedDescription` is the server's or the system's prose and
    /// nobody has checked what is in it.
    enum Failure: String, Sendable {
        case transport
        case unauthorized
        case forbidden
        case notAdmitted
        case notFound
        case badInput
        case server
        case decoding
        case database
        case other
    }

    /// What this reads as in the file and in Console.
    var written: String {
        switch self {
        case .count(let n): String(n)
        case .id(let n): String(n)
        case .milliseconds(let n): "\(n)ms"
        case .flag(let yes): yes ? "true" : "false"
        case .route(let route): route.rawValue
        case .outcome(let outcome): outcome.rawValue
        case .request(let outcome): outcome.rawValue
        case .failure(let failure): failure.rawValue
        case .word(let word): String(describing: word)
        }
    }
}

extension Plain.Failure {
    /// Classifies an error without quoting it.
    ///
    /// Deliberately narrow. `APIError.badInput` carries a sentence the server wrote and
    /// that sentence is allowed to name a list; `URLError` carries the URL it failed
    /// on. Neither may reach a line at `info` or above, so what survives is the shape.
    init(_ error: Error) {
        switch error {
        case let api as APIError:
            switch api {
            case .unauthorized: self = .unauthorized
            case .forbidden: self = .forbidden
            case .notAdmitted: self = .notAdmitted
            case .notFound: self = .notFound
            case .badInput: self = .badInput
            case .server: self = .server
            case .transport: self = .transport
            }
        case is DecodingError:
            self = .decoding
        case let url as URLError where url.code == .cancelled:
            self = .transport
        case is URLError:
            self = .transport
        default:
            self = .other
        }
    }
}

/// One named value on a log line.
///
/// The name is a ``StaticString`` for the same reason the message is: a key built from
/// a variable is a key that could be an item name.
struct Detail: Sendable {
    let name: StaticString
    let value: Plain

    init(_ name: StaticString, _ value: Plain) {
        self.name = name
        self.value = value
    }

    var written: String { "\(name)=\(value.written)" }
}

// MARK: - Writing

/// Where log lines go, and how much of them.
///
/// One of these per process, normally ``shared``. Tests make their own so that two of
/// them running at once do not read each other's file, and so that turning the level up
/// in one does not turn it up everywhere.
///
/// Everything here is safe to call from any thread and from any actor: logging that has
/// to be `await`ed is logging that does not get added to the paths worth watching.
final class LogBook: @unchecked Sendable {

    /// The process's book. Read by the ``Log`` free functions, which is what almost
    /// every call site uses.
    static let shared = LogBook(file: .shared)

    /// Nothing is written above this.
    ///
    /// **`warn` is the default, and that is what "logging is off" means here.** A
    /// warning and an error are written whether or not anybody asked, because the only
    /// alternative is an app that says nothing at all about a failure until somebody
    /// reproduces it with the switch on — which is the failure nobody can reproduce.
    /// Everything below is off until somebody turns it on in Settings.
    var level: LogLevel {
        get { lock.withLock { held } }
        set { lock.withLock { held = newValue } }
    }

    private var held: LogLevel
    private let lock = NSLock()
    private let file: LogFile?
    private let loggers: [LogArea: Logger]

    init(file: LogFile?, level: LogLevel = LogSettings.level, subsystem: String = LogSettings.subsystem) {
        self.file = file
        self.held = level
        self.loggers = Dictionary(
            uniqueKeysWithValues: LogArea.allCases.map {
                ($0, Logger(subsystem: subsystem, category: $0.rawValue))
            }
        )
    }

    // MARK: - The three that may not carry personal data

    func info(_ area: LogArea, _ message: StaticString, _ details: Detail...) {
        write(.info, area, String(describing: message), details)
    }

    func warn(_ area: LogArea, _ message: StaticString, _ details: Detail...) {
        write(.warn, area, String(describing: message), details)
    }

    func error(_ area: LogArea, _ message: StaticString, _ details: Detail...) {
        write(.error, area, String(describing: message), details)
    }

    // MARK: - The two that may say anything

    /// May name items and lists, and usually does. See ``Log``.
    func debug(_ area: LogArea, _ message: @autoclosure () -> String) {
        guard level <= .debug else { return }
        write(.debug, area, message(), [])
    }

    /// May name items and lists, and usually does. See ``Log``.
    ///
    /// An autoclosure, so building the sentence costs nothing when nobody asked for it.
    /// A trace line that interpolates a whole list is exactly the line that should not
    /// be built forty times a second on a phone with tracing off.
    func trace(_ area: LogArea, _ message: @autoclosure () -> String) {
        guard level <= .trace else { return }
        write(.trace, area, message(), [])
    }

    private func write(_ at: LogLevel, _ area: LogArea, _ message: String, _ details: [Detail]) {
        guard at >= level else { return }

        let line = details.isEmpty
            ? message
            : message + " " + details.map(\.written).joined(separator: " ")

        // `.public`, and deliberately. Unified logging redacts dynamic strings by
        // default, which is the right default for an app that logs whatever it has to
        // hand -- and the wrong one here, where what may be dynamic at all has already
        // been decided by the type of the call. A redacted line reads `<private>` in
        // Console and in a sysdiagnose, which makes the levels somebody turned on for a
        // reason useless to them.
        loggers[area]?.log(level: at.osLevel, "\(line, privacy: .public)")

        file?.append(at, area, line)
    }
}

/// The process's log, as everything outside a test calls it.
///
/// Five levels, in two groups, and the split is the point:
///
/// * ``trace(_:_:)`` and ``debug(_:_:)`` take a `String` and **may say anything**,
///   including what is on a list. Both are off until somebody turns them on, and the
///   screen that turns them on says so first.
/// * ``info(_:_:_:)``, ``warn(_:_:_:)`` and ``error(_:_:_:)`` take a `StaticString` and
///   ``Detail``s, and **cannot** carry personal data: there is no expression that puts
///   a `String` into either half. See ``Plain``.
///
/// That is the whole redaction boundary. It is a pair of signatures rather than a rule
/// in a document, because a rule in a document is checked by whoever remembers it.
enum Log {
    static func info(_ area: LogArea, _ message: StaticString, _ details: Detail...) {
        LogBook.shared.write(.info, area, message, details)
    }

    static func warn(_ area: LogArea, _ message: StaticString, _ details: Detail...) {
        LogBook.shared.write(.warn, area, message, details)
    }

    static func error(_ area: LogArea, _ message: StaticString, _ details: Detail...) {
        LogBook.shared.write(.error, area, message, details)
    }

    static func debug(_ area: LogArea, _ message: @autoclosure () -> String) {
        LogBook.shared.debug(area, message())
    }

    static func trace(_ area: LogArea, _ message: @autoclosure () -> String) {
        LogBook.shared.trace(area, message())
    }
}

extension LogBook {
    /// The variadic forwarders above have already collected their arguments, so this is
    /// what they hand over. Not for anything else to call.
    fileprivate func write(
        _ at: LogLevel,
        _ area: LogArea,
        _ message: StaticString,
        _ details: [Detail]
    ) {
        write(at, area, String(describing: message), details)
    }
}

// MARK: - Where the level is kept

/// The stored answer to "how much should this app write down".
///
/// `UserDefaults` and not a `@AppStorage`, because the things that log are backends,
/// queues and session delegates rather than views — and because the watch is told its
/// level by the phone rather than reading one of its own.
enum LogSettings {
    /// The reverse-DNS name every line is filed under in Console.
    static let subsystem = "dev.f28956.shopping-list"

    private static let key = "diagnostics.level"

    /// What was chosen, or ``LogLevel/warn`` — which is what off means. See
    /// ``LogBook/level``.
    static var level: LogLevel {
        get {
            guard let stored = UserDefaults.standard.string(forKey: key),
                  let level = LogLevel(stored: stored)
            else { return .warn }
            return level
        }
        set {
            UserDefaults.standard.set(newValue.stored, forKey: key)
            // The book is what everything actually reads, and nothing observes
            // `UserDefaults` -- the same reason `ServerDirectory` announces.
            LogBook.shared.level = newValue
            NotificationCenter.default.post(name: .diagnosticsChanged, object: nil)
        }
    }
}

extension Notification.Name {
    /// How much is being logged, or where metrics go, has changed.
    ///
    /// What tells the watch: the phone pushes its level across the link when this
    /// fires, because nobody sets a log level on a wrist.
    static let diagnosticsChanged = Notification.Name("shoppinglist.diagnosticsChanged")
}
