//! The numbers this server keeps, and the only ways to move them.
//!
//! Every instrument is private and every caller goes through a function here. That is
//! not ceremony: a metric's labels are the part that is expensive to get wrong, and
//! the mistake is always the same one — labelling by the thing you were looking at.
//! `list_id`, `item`, `email` and `token` are all natural things to reach for and all
//! of them turn a handful of series into one per row, per household, for ever. None of
//! the signatures below will take one.
//!
//! Names follow OpenTelemetry's dotted convention. The Prometheus exporter rewrites
//! them to `http_server_requests_total` and friends on the way out, so both audiences
//! see the spelling they expect from one definition.

use std::sync::LazyLock;
use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram, UpDownCounter};

/// What the meter is called in every exported series.
const SCOPE: &str = "shopping-list";

/// The instruments, built once against whatever meter provider is installed.
///
/// Every field is private. A caller reaches them through the functions below, and
/// none of those will take a row — see the module header.
pub struct Instruments {
    http_requests: Counter<u64>,
    http_duration: Histogram<f64>,
    http_in_flight: UpDownCounter<i64>,

    sync_batches: Counter<u64>,
    sync_batch_size: Histogram<u64>,
    sync_operations: Counter<u64>,

    sse_subscribers: UpDownCounter<i64>,
    sse_duration: Histogram<f64>,

    db_query_duration: Histogram<f64>,
    db_pool: Gauge<u64>,

    sign_ins: Counter<u64>,
    admissions_refused: Counter<u64>,
    invite_redemptions: Counter<u64>,
}

static INSTRUMENTS: LazyLock<Instruments> = LazyLock::new(|| {
    let meter = opentelemetry::global::meter(SCOPE);

    Instruments {
        http_requests: meter
            .u64_counter("http.server.requests")
            .with_description("Requests answered, by route pattern, method and status")
            .build(),
        http_duration: meter
            .f64_histogram("http.server.duration")
            .with_description("How long a request took, from arrival to response")
            .with_unit("s")
            .build(),
        http_in_flight: meter
            .i64_up_down_counter("http.server.active_requests")
            .with_description("Requests being served right now")
            .build(),

        sync_batches: meter
            .u64_counter("sync.batches")
            .with_description("Offline batches replayed, and whether each ran to the end")
            .build(),
        sync_batch_size: meter
            .u64_histogram("sync.batch.size")
            .with_description("Operations per batch, which is how far behind a device was")
            .with_unit("{operation}")
            .build(),
        sync_operations: meter
            .u64_counter("sync.operations")
            .with_description("Replayed operations by kind, outcome and reason for refusal")
            .build(),

        sse_subscribers: meter
            .i64_up_down_counter("sse.subscribers")
            .with_description("Event streams open right now")
            .build(),
        sse_duration: meter
            .f64_histogram("sse.stream.duration")
            .with_description("How long an event stream stayed open")
            .with_unit("s")
            .build(),

        db_query_duration: meter
            .f64_histogram("db.query.duration")
            .with_description("Statement execution time, by verb")
            .with_unit("s")
            .build(),
        db_pool: meter
            .u64_gauge("db.pool.connections")
            .with_description("Pooled connections, held and idle")
            .build(),

        sign_ins: meter
            .u64_counter("auth.sign_ins")
            .with_description("Sign-ins by provider and outcome")
            .build(),
        admissions_refused: meter
            .u64_counter("auth.admissions_refused")
            .with_description("Callers turned away because they are not admitted")
            .build(),
        invite_redemptions: meter
            .u64_counter("invites.redemptions")
            .with_description("Share links followed, and whether they worked")
            .build(),
    }
});

/// The instruments. See [`Instruments`] for why the moment this is first touched
/// matters.
pub fn instruments() -> &'static Instruments {
    &INSTRUMENTS
}

/// Builds the instruments now, so that the no-op provider cannot be captured later.
///
/// Called by `server::metrics::install` immediately after the real provider is in
/// place. Separate from `instruments()` so the ordering requirement has a name in the
/// call site rather than being a comment somebody has to find.
pub fn warm_up() {
    LazyLock::force(&INSTRUMENTS);
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

impl Instruments {
    /// One answered request.
    ///
    /// `route` is the matched pattern and never the path: `/api/lists/{list_id}/items`
    /// and not `/api/lists/4108/items`. The second would put one time series per list
    /// into the scrape output, which is a cardinality problem and a disclosure — a
    /// scrape of an unprotected endpoint would enumerate every list on the server.
    pub fn request(&self, route: &str, method: &str, status: u16, seconds: f64) {
        // Owned, because `route` borrows the request's extensions and the labels
        // outlive the call. `method` and the status are small closed sets.
        let labels = [
            KeyValue::new("route", route.to_string()),
            KeyValue::new("method", method.to_string()),
            KeyValue::new("status", i64::from(status)),
        ];

        self.http_requests.add(1, &labels);
        self.http_duration.record(seconds, &labels);
    }

    pub fn request_started(&self) {
        self.http_in_flight.add(1, &[]);
    }

    pub fn request_finished(&self) {
        self.http_in_flight.add(-1, &[]);
    }
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

/// What `POST /api/sync` did with a batch.
///
/// Takes the kinds the device sent alongside the answers rather than the operations
/// themselves, because the operations are consumed by the service and because the kind
/// is the only part of one that is safe to label with — the rest is somebody's
/// shopping.
///
/// A batch that stopped early is counted as such and its unanswered tail is not
/// invented: `replay` hands back what landed and stops, so the difference between
/// `sent` and the answers is the tail the device will send again. That difference is
/// the symptom of the failure mode that `sync::replay` was changed to avoid, and it is
/// worth being able to see it on a graph.
pub fn sync_replayed(kinds: &[&'static str], applied: &[domain::service::sync::Applied]) {
    use domain::service::sync::{Outcome, Refusal};

    let instruments = instruments();

    let complete = applied.len() == kinds.len();
    instruments.sync_batches.add(
        1,
        &[KeyValue::new(
            "result",
            if complete { "complete" } else { "stopped_early" },
        )],
    );
    instruments
        .sync_batch_size
        .record(kinds.len() as u64, &[]);

    for (kind, answer) in kinds.iter().zip(applied) {
        let (outcome, refusal) = match &answer.outcome {
            Outcome::Applied { .. } => ("applied", "none"),
            Outcome::AlreadyApplied { .. } => ("already_applied", "none"),
            Outcome::Refused { why } => (
                "refused",
                match why {
                    Refusal::Gone => "gone",
                    Refusal::ListGone => "list_gone",
                    Refusal::NotAllowed => "not_allowed",
                    Refusal::Invalid => "invalid",
                },
            ),
        };

        instruments.sync_operations.add(
            1,
            &[
                KeyValue::new("kind", *kind),
                KeyValue::new("outcome", outcome),
                KeyValue::new("refusal", refusal),
            ],
        );
    }
}

// ---------------------------------------------------------------------------
// Event streams
// ---------------------------------------------------------------------------

/// One open event stream, counted for as long as it is held.
///
/// A guard rather than a pair of calls. An SSE handler returns a stream and then stops
/// running, so "when did this end" is a question only `Drop` can answer — a decrement
/// written at the end of the handler would fire while every subscriber was still
/// connected, and the gauge would sit at zero for ever.
pub struct SseStream {
    transport: &'static str,
    opened: Instant,
}

impl SseStream {
    /// `transport` is `"api"` or `"web"`. Not the list — see the module header.
    pub fn opened(transport: &'static str) -> Self {
        instruments()
            .sse_subscribers
            .add(1, &[KeyValue::new("transport", transport)]);
        SseStream { transport, opened: Instant::now() }
    }
}

impl Drop for SseStream {
    fn drop(&mut self) {
        let labels = [KeyValue::new("transport", self.transport)];
        instruments().sse_subscribers.add(-1, &labels);
        instruments()
            .sse_duration
            .record(self.opened.elapsed().as_secs_f64(), &labels);
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// One statement, timed.
///
/// `verb` is `SELECT`, `INSERT`, `UPDATE`, `DELETE` or `OTHER` and nothing else, which
/// [`verb_of`] enforces. The statement text itself is not a label: it would be
/// hundreds of series, and a parameterised statement is not much of a secret but the
/// habit of putting query text in labels is how bound values eventually get there too.
pub fn db_query(verb: &'static str, seconds: f64) {
    instruments()
        .db_query_duration
        .record(seconds, &[KeyValue::new("verb", verb)]);
}

/// The verb a statement starts with, collapsed to a closed set.
///
/// Anything unrecognised is `OTHER` rather than passed through, so a label can never be
/// something a caller influenced.
pub fn verb_of(statement: &str) -> &'static str {
    let first = statement
        .trim_start()
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or("");

    match first.to_ascii_uppercase().as_str() {
        "SELECT" => "SELECT",
        "INSERT" | "REPLACE" => "INSERT",
        "UPDATE" => "UPDATE",
        "DELETE" => "DELETE",
        _ => "OTHER",
    }
}

/// How the connection pool is doing.
///
/// `size` is every connection the pool holds and `idle` the ones nobody is using, so
/// the difference is the number of requests waiting on SQLite's single writer. A pool
/// pinned at zero idle is the shape a self-hoster sees as "the app got slow", and it is
/// invisible without this.
pub fn db_pool(size: u32, idle: usize) {
    let held = u64::from(size).saturating_sub(idle as u64);
    instruments()
        .db_pool
        .record(held, &[KeyValue::new("state", "in_use")]);
    instruments()
        .db_pool
        .record(idle as u64, &[KeyValue::new("state", "idle")]);
}

// ---------------------------------------------------------------------------
// Getting in
// ---------------------------------------------------------------------------

/// A sign-in, by provider and by what happened.
///
/// `provider` is the stable name a `Provider` carries — `"google"`, `"apple"` — which
/// is configuration and not anybody's data.
pub fn sign_in(provider: &'static str, outcome: SignIn) {
    instruments().sign_ins.add(
        1,
        &[
            KeyValue::new("provider", provider),
            KeyValue::new("outcome", outcome.label()),
        ],
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    /// A session token was issued.
    Issued,
    /// The token was good and this server does not admit them.
    NotAdmitted,
    /// The token itself did not verify.
    Rejected,
}

impl SignIn {
    fn label(self) -> &'static str {
        match self {
            SignIn::Issued => "issued",
            SignIn::NotAdmitted => "not_admitted",
            SignIn::Rejected => "rejected",
        }
    }
}

/// Somebody turned away because they are not on the list.
///
/// `at` says which door: `"bearer"` for an API caller, `"session"` for a browser whose
/// admission was withdrawn while they were signed in. The second is the one worth
/// watching, because it is what a removal is supposed to produce and its absence means
/// removal is not taking effect.
pub fn admission_refused(at: &'static str) {
    instruments()
        .admissions_refused
        .add(1, &[KeyValue::new("at", at)]);
}

/// A share link followed. The token is not a label and never will be — it is a bearer
/// credential with seven days left on it.
pub fn invite_redeemed(joined: bool) {
    instruments().invite_redemptions.add(
        1,
        &[KeyValue::new(
            "outcome",
            if joined { "joined" } else { "refused" },
        )],
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    /// The label is a closed set, so nothing a caller wrote can become one.
    #[rstest]
    #[case("SELECT 1 FROM items", "SELECT")]
    #[case("\n  select id from lists\n", "SELECT")]
    #[case("INSERT INTO items (name) VALUES (?)", "INSERT")]
    #[case("REPLACE INTO tag_order VALUES (?)", "INSERT")]
    #[case("UPDATE items SET done = 1", "UPDATE")]
    #[case("DELETE FROM sessions", "DELETE")]
    #[case("PRAGMA foreign_keys = ON", "OTHER")]
    #[case("", "OTHER")]
    #[case("-- a comment", "OTHER")]
    fn a_statement_is_labelled_by_its_verb_and_nothing_else(
        #[case] statement: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(super::verb_of(statement), expected);
    }
}
