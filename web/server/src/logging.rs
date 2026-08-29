//! What the log says, how loudly, and in what shape.
//!
//! Three things are configurable and one is not.
//!
//! Configurable: the level (`LOG_LEVEL`, down to `trace`), the shape (`LOG_FORMAT`,
//! `text` or `json`, because a self-hoster may ship the log somewhere that wants to
//! parse it), and `RUST_LOG` for anybody who wants per-crate directives.
//!
//! Not configurable: **`info`, `warn` and `error` never carry the contents of
//! anybody's lists.** That is enforced here rather than left to whoever writes the
//! next log line. Contents go through `observability::contents!`, which can only
//! produce an event on the `contents` target; that target is carried by its own layer,
//! switched by one boolean, and dropped outright by the layer everything else goes
//! through. The same boolean decides whether [`Settings::announce`] warns — so there
//! is no configuration in which a log holds somebody's shopping and the process did
//! not say at startup that it would.
//!
//! `docs/self-hosting.md` S8 is why. The person with root is the user, so this is not
//! about hiding things from an operator; it is about the copy of the log that gets
//! pasted into an issue, shipped to a hosted log service, or left on a disk. A
//! shopping list is more revealing than it looks — medication, dietary restrictions,
//! pregnancy tests — and none of it should be in a file whose whole purpose is to be
//! sent to somebody else.

use std::fmt;

use anyhow::Context;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context as LayerContext, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, filter, registry::LookupSpan};

/// The filter used when nobody said anything, preserved exactly as it was.
///
/// `domain` is here because the interesting lines are: sign-in refused, the server has
/// been claimed, a shared list changed hands, account closed. Without it a self-hoster
/// reading the log to find out why somebody cannot get in sees every request and no
/// answer.
///
/// At `info`, not `debug`, because the service layer is on the hot path and its debug
/// is per-query noise.
const DEFAULT: &str =
    "server=debug,api=debug,web=debug,observability=debug,domain=info,tower_http=debug,sqlx=warn";

/// How a line is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// One human-readable line. The default, because the first reader of this log is
    /// the person who just started the process.
    Text,
    /// One JSON object per line, for a self-hoster who ships the log somewhere that
    /// parses it. Offered rather than assumed: a person watching a terminal is worse
    /// off reading JSON, and that is the common case.
    Json,
}

/// Everything the subscriber needs to know.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Filter directives, in `EnvFilter`'s language. Already validated: reaching here
    /// means they parse.
    pub directives: String,
    pub format: Format,
    /// Whether `RUST_LOG` was set and lost to `LOG_LEVEL`. Said out loud at startup,
    /// because a stale `RUST_LOG` in a shell that quietly beats the variable somebody
    /// just set is a confusing half-hour.
    pub rust_log_ignored: bool,
}

impl Settings {
    /// Reads the environment, refusing what cannot work rather than starting and
    /// logging nothing.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::read(|name| std::env::var(name).ok())
    }

    /// The same, over any source of values. Split out so the rules can be tested
    /// without a process-wide environment that other tests are also reading — the same
    /// arrangement `tls::Settings::read` uses.
    pub fn read(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let rust_log = get("RUST_LOG").filter(|value| !value.trim().is_empty());

        // `LOG_LEVEL` wins where both are set. It is the documented knob and the one a
        // person sets on purpose; `RUST_LOG` is frequently left over from an earlier
        // session in the same shell.
        let log_level = get("LOG_LEVEL").filter(|value| !value.trim().is_empty());
        let directives = match &log_level {
            Some(level) => directives_for(level.trim())?,
            None => rust_log.clone().unwrap_or_else(|| DEFAULT.to_string()),
        };

        // Parsed here rather than at `init`, so a typo is a refusal at startup and not
        // a process that runs for a month with a filter nobody checked.
        EnvFilter::builder()
            .parse(&directives)
            .with_context(|| format!("LOG_LEVEL is \"{directives}\", which is not a filter"))?;

        let format = match get("LOG_FORMAT").as_deref().map(str::trim) {
            None | Some("") | Some("text") => Format::Text,
            Some("json") => Format::Json,
            Some(other) => anyhow::bail!("LOG_FORMAT is \"{other}\"; it must be text or json"),
        };

        Ok(Settings {
            rust_log_ignored: rust_log.is_some() && log_level.is_some(),
            directives,
            format,
        })
    }

    /// Whether anything at all is admitted at `debug` or below.
    ///
    /// The single question that decides whether contents may be logged. Asked of the
    /// parsed filter rather than of the string, so `api=debug` buried in the middle of
    /// a `RUST_LOG` directive list counts — a rule that only noticed the word `debug`
    /// at the front would be a rule with a hole in it.
    pub fn verbose(&self) -> bool {
        EnvFilter::new(&self.directives).max_level_hint() >= Some(filter::LevelFilter::DEBUG)
    }

    /// Installs the subscriber. Called once, before anything else logs.
    pub fn install(&self) -> anyhow::Result<()> {
        self.subscriber(std::io::stdout).init();
        Ok(())
    }

    /// The subscriber, over any destination.
    ///
    /// Split from [`Settings::install`] for the same reason [`Settings::read`] is
    /// split from `from_env`: the rule worth testing is what comes out, and a test
    /// cannot read a process's stdout. The one caller in production passes
    /// `std::io::stdout`.
    pub fn subscriber<W>(&self, writer: W) -> impl tracing::Subscriber + Send + Sync
    where
        W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Clone + Send + Sync + 'static,
    {
        let verbose = self.verbose();

        // Two layers over the same destination, and the split is the whole mechanism.
        //
        // The first carries everything the operator asked for and can never carry
        // contents: its second filter drops that target outright, whatever the
        // directives say.
        //
        // The second carries contents and nothing else, and is switched by exactly one
        // boolean — the same one that decides whether `announce` warns. So there is no
        // configuration in which a log holds somebody's shopping without the process
        // having said, at startup, that it would. A single stack of filters could not
        // do this: `debug` as a global level would still drop a `trace` event, and
        // `contents=trace` as a directive would be a switch of its own, reachable from
        // `RUST_LOG` and disconnected from the warning.
        let ordinary = self
            .format(writer.clone())
            .with_filter(EnvFilter::new(&self.directives))
            .with_filter(filter::filter_fn(|meta| {
                meta.target() != observability::CONTENTS
            }));

        let contents = self
            .format(writer)
            .with_filter(filter::filter_fn(move |meta| {
                verbose && meta.target() == observability::CONTENTS
            }));

        tracing_subscriber::registry()
            .with(ordinary)
            .with(contents)
            .with(db_queries())
    }

    /// One layer in the configured shape.
    ///
    /// Boxed because the two shapes are different types and only one of them is
    /// wanted. The cost is one virtual call per event, on a path that has already
    /// decided to write to a file descriptor.
    fn format<S, W>(&self, writer: W) -> Box<dyn Layer<S> + Send + Sync>
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a>,
        W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
    {
        match self.format {
            Format::Text => tracing_subscriber::fmt::layer().with_writer(writer).boxed(),
            // Flattened, and without the span list: a line that has to be unwrapped
            // twice before the message is visible is worse in every log viewer.
            Format::Json => tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .boxed(),
        }
    }

    /// Said at startup, in the same breath as `TLS_MODE=off`.
    ///
    /// Loud and unstructured, like the claim code in `main`, and for the same reason:
    /// this is a line whose whole job is to be read by the person who just started the
    /// process, and a field buried in JSON is a field they will miss.
    pub fn announce(&self) {
        if self.rust_log_ignored {
            tracing::info!("RUST_LOG is set and LOG_LEVEL wins; RUST_LOG is being ignored");
        }

        if self.verbose() {
            tracing::warn!("");
            tracing::warn!("  This log will contain the contents of people's lists.");
            tracing::warn!("  At debug and trace nothing is held back: item names, list names,");
            tracing::warn!("  addresses and invite codes are all written out.");
            tracing::warn!("  Do not ship it anywhere, and delete it when you are done.");
            tracing::warn!("");
        }
    }
}

/// Expands a bare level into directives, or takes directives as given.
///
/// A person who writes `LOG_LEVEL=debug` means "tell me more", and a person who writes
/// `LOG_LEVEL=api=debug,domain=trace` means exactly that. Telling them apart by
/// looking for a `=` rather than offering two variables keeps the common case one
/// word.
fn directives_for(level: &str) -> anyhow::Result<String> {
    if level.contains('=') || level.contains(',') {
        return Ok(level.to_string());
    }

    Ok(match level {
        // `trace` means everything, dependencies included. Somebody who asked for this
        // is looking for a bug in a library, and holding parts of it back would hide
        // the half they are looking for.
        "trace" => "trace".to_string(),
        // At `debug` the chatty dependencies are held at `info`. Their debug is
        // per-packet and per-statement, and it drowns the application's — which is
        // what the person actually asked to see.
        "debug" => {
            "debug,sqlx=warn,hyper=info,hyper_util=info,h2=info,rustls=info,reqwest=info".to_string()
        }
        "info" | "warn" | "error" => level.to_string(),
        other => anyhow::bail!(
            "LOG_LEVEL is \"{other}\"; it must be error, warn, info, debug or trace, \
             or a list of directives such as \"api=debug,domain=info\""
        ),
    })
}

/// Turns sqlx's own log lines into a latency histogram.
///
/// A layer rather than instrumenting the queries, because the queries are in `domain`
/// and `domain` is compiled into phones — see `observability`'s manifest. sqlx already
/// emits one event per statement with its elapsed time on it, and reading those is how
/// the server gets database timings without any of the metrics machinery reaching a
/// crate that must not have it.
///
/// Filtered to `sqlx::query` alone and at `debug`, independently of what the operator
/// asked to *see*. That separation is deliberate: measuring every statement and
/// printing every statement are different requests, and a server should not have to
/// run at `debug` to have a latency graph.
fn db_queries<S>() -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    DbQueries.with_filter(EnvFilter::new("sqlx::query=debug"))
}

struct DbQueries;

impl<S: tracing::Subscriber> Layer<S> for DbQueries {
    fn on_event(&self, event: &tracing::Event<'_>, _: LayerContext<'_, S>) {
        let mut query = Query::default();
        event.record(&mut query);

        if let Some(seconds) = query.seconds {
            observability::instruments::db_query(query.verb, seconds);
        }
    }
}

/// The two fields of a sqlx query event this cares about.
///
/// `db.statement` is deliberately not one of them. It is the SQL text, which carries
/// no bound values and is still not something to turn into a metric label — see
/// `observability::instruments::db_query`.
struct Query {
    seconds: Option<f64>,
    verb: &'static str,
}

impl Default for Query {
    fn default() -> Self {
        Query { seconds: None, verb: "OTHER" }
    }
}

impl Visit for Query {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "elapsed_secs" {
            self.seconds = Some(value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "summary" {
            self.verb = observability::instruments::verb_of(value);
        }
    }

    /// The fallback, because whether a `String` field arrives as a string or as its
    /// `Debug` is a detail of the version of `tracing` that compiled the call site.
    /// Quotes are stripped so `"SELECT …"` still starts with a verb.
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "summary" {
            let rendered = format!("{value:?}");
            self.verb = observability::instruments::verb_of(rendered.trim_matches('"'));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use rstest::rstest;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    fn settings_from(vars: &[(&str, &str)]) -> anyhow::Result<Settings> {
        let vars: std::collections::HashMap<_, _> = vars.iter().copied().collect();
        Settings::read(|name| vars.get(name).map(|v| v.to_string()))
    }

    /// A server nobody configured logs what it logged before this module existed.
    /// Adding a knob should not change the answer for everybody who did not touch it.
    #[test]
    fn nothing_configured_logs_what_it_always_did() {
        let settings = settings_from(&[]).unwrap();

        assert_eq!(settings.directives, DEFAULT);
        assert_eq!(settings.format, Format::Text);
        assert!(!settings.rust_log_ignored);
    }

    #[test]
    fn a_bare_level_is_enough_and_directives_are_taken_as_given() {
        assert_eq!(settings_from(&[("LOG_LEVEL", "warn")]).unwrap().directives, "warn");
        assert_eq!(
            settings_from(&[("LOG_LEVEL", "api=debug,domain=trace")]).unwrap().directives,
            "api=debug,domain=trace"
        );
    }

    /// `LOG_LEVEL` wins, and the loss is said out loud. A stale `RUST_LOG` in a shell
    /// that silently beats the variable somebody just set is a confusing half-hour.
    #[test]
    fn log_level_beats_rust_log_and_the_loss_is_announced() {
        let both = settings_from(&[("LOG_LEVEL", "warn"), ("RUST_LOG", "trace")]).unwrap();
        assert_eq!(both.directives, "warn");
        assert!(both.rust_log_ignored);

        let only_rust_log = settings_from(&[("RUST_LOG", "api=trace")]).unwrap();
        assert_eq!(only_rust_log.directives, "api=trace");
        assert!(!only_rust_log.rust_log_ignored);
    }

    /// Refused at startup rather than at the first line nobody sees.
    #[rstest]
    #[case::not_a_level(&[("LOG_LEVEL", "verbose")])]
    #[case::not_a_level_in_directives(&[("LOG_LEVEL", "api=chatty")])]
    #[case::not_a_format(&[("LOG_FORMAT", "xml")])]
    fn configuration_that_cannot_work_is_refused(#[case] vars: &[(&str, &str)]) {
        assert!(settings_from(vars).is_err(), "{vars:?} was accepted");
    }

    /// The question the contents rule turns on, asked of the parsed filter and not of
    /// the string — so a `debug` buried in a directive list counts.
    #[rstest]
    #[case("error", false)]
    #[case("warn", false)]
    #[case("info", false)]
    #[case("debug", true)]
    #[case("trace", true)]
    #[case("info,api=debug", true)]
    #[case("warn,domain=trace", true)]
    fn verbosity_is_read_from_the_whole_filter(#[case] level: &str, #[case] expected: bool) {
        assert_eq!(
            settings_from(&[("LOG_LEVEL", level)]).unwrap().verbose(),
            expected,
            "{level} was read wrong"
        );
    }

    /// Somewhere to write, so a test can read what came out.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn logged(vars: &[(&str, &str)]) -> String {
        let captured = Captured::default();
        let settings = settings_from(vars).unwrap();

        // Scoped rather than installed, so each case gets its own subscriber and these
        // tests do not have to run in an order.
        tracing::subscriber::with_default(settings.subscriber(captured.clone()), || {
            tracing::info!(list = 4108, items = 3, "list read");
            observability::contents!(bought = "pregnancy test", "added");
        });

        captured.text()
    }

    /// The headline rule, end to end: at `info` the log holds the shape and not the
    /// shopping.
    #[test]
    fn contents_are_absent_below_debug() {
        let written = logged(&[("LOG_LEVEL", "info")]);

        assert!(written.contains("list read"), "the ordinary line is missing: {written}");
        assert!(
            !written.contains("pregnancy test"),
            "an item name reached the log at info: {written}"
        );
    }

    /// And present when somebody asked for it, having been told what it means.
    #[test]
    fn contents_are_written_at_trace() {
        let written = logged(&[("LOG_LEVEL", "trace")]);

        assert!(written.contains("pregnancy test"), "trace held something back: {written}");
    }

    /// What makes the rule structural rather than a convention: contents appear if and
    /// only if the startup warning fired. Both are `verbose()` and there is no third
    /// way to reach the contents target — so no directive, however specific, produces
    /// a log holding somebody's shopping without first saying that it will.
    ///
    /// The case worth having: `contents=trace` named explicitly. Without the second
    /// filter that directive would be its own switch, reachable from `RUST_LOG` and
    /// disconnected from the warning.
    #[rstest]
    #[case(&[("LOG_LEVEL", "error")])]
    #[case(&[("LOG_LEVEL", "info")])]
    #[case(&[("LOG_LEVEL", "debug")])]
    #[case(&[("LOG_LEVEL", "trace")])]
    #[case(&[("RUST_LOG", "info")])]
    #[case(&[("RUST_LOG", "info,contents=trace")])]
    #[case(&[("RUST_LOG", "warn,api=debug")])]
    #[case(&[])]
    fn contents_are_written_only_where_the_warning_fired(#[case] vars: &[(&str, &str)]) {
        let settings = settings_from(vars).unwrap();
        let written = logged(vars);

        assert_eq!(
            written.contains("pregnancy test"),
            settings.verbose(),
            "{vars:?} logged contents without warning about it, or warned for nothing"
        );
    }

    #[test]
    fn json_is_selectable_for_an_operator_who_ships_the_log_elsewhere() {
        let written = logged(&[("LOG_FORMAT", "json"), ("LOG_LEVEL", "info")]);
        let first = written.lines().next().expect("nothing was written");

        let parsed: serde_json::Value =
            serde_json::from_str(first).unwrap_or_else(|e| panic!("{first} is not JSON: {e}"));

        assert_eq!(parsed["message"], "list read");
        assert_eq!(parsed["list"], 4108);
    }
}
