import Foundation

/// Reports a database call that failed, and carries on.
///
/// Loud where somebody is watching, quiet where nobody can act on it. A cache is a copy
/// and losing a write to it costs a re-read rather than somebody's shopping, so shipping
/// a crash would be the wrong trade — but a bare `try?` in a *debug* build is how
/// `adopt(_: as:)` shipped having never once worked. Every statement was handed three
/// arguments while only the first named three, GRDB refused, the transaction rolled
/// back, and a list made offline kept its old id, its items, its walking order, its
/// history and its queue all pointing at a number the server had already replaced. The
/// tests were green throughout, because nothing called it and nothing said a word.
///
/// The rule this encodes: **a swallowed error is a decision, and it has to be made
/// once, in the open.** `try?` makes it twenty times, invisibly.
///
/// - Note: The `queue == nil` case is not a failure and does not come through here. A
///   database that was never opened is a state the callers handle deliberately; this is
///   only for one that was opened and then refused a statement.
/// - Parameter what: a `StaticString`, so it can only be a word somebody typed here.
///   That is the same rule the log's `info` lines follow and it is the reason this can
///   be written down at `error` without anybody checking the call sites: see ``Plain``.
func noted(
    _ error: Error,
    _ what: StaticString,
    file: StaticString = #fileID,
    line: UInt = #line
) {
    // In every build, before the assertion. A release build kept the only record of this
    // in a `print` nobody is attached to, and a debug build stopped at the assertion --
    // so on a phone in somebody's pocket, the case this note was written about, it was
    // recorded nowhere at all.
    Log.error(.store, "a statement failed", Detail("doing", .word(what)), Detail("why", .failure(Plain.Failure(error))))
    Metrics.shared.count(Measured.storeFailed, Tagged("doing", .word(what)))
    // The error's own words, which may name a row and so may not go above `debug`.
    Log.debug(.store, "\(what) failed: \(error)")

    #if DEBUG
        // Stops the run, and stops the test suite, which is the point: every one of
        // these is a bug in a statement rather than bad luck at runtime.
        assertionFailure("[store] \(what) failed: \(error)", file: file, line: line)
    #endif
}
