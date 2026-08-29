//! The rule from `server::logging`, checked against every log site in the workspace.
//!
//! `info`, `warn` and `error` must never carry the contents of anybody's lists: no
//! item names, no list names, no addresses, no session tokens, no invite codes. Counts,
//! shapes, ids, durations, status codes and outcomes are all fine.
//!
//! A test rather than a review note. The rule is easy to state and easy to forget, and
//! the cost of forgetting is a log file — the copy of this data most likely to be
//! pasted into an issue, shipped to a hosted log service, or left on a sold disk —
//! holding what a household bought. `docs/self-hosting.md` S8 is the reasoning.
//!
//! A log site that needs to say what somebody typed has somewhere to go:
//! `observability::contents!`, which cannot emit anywhere the operator has not been
//! warned about first.
//!
//! **Why this is a separate test binary rather than a module inside `logging.rs`.** It
//! reads Rust source and looks for macro names in it, so scanning the crate that
//! contains it would find its own string literals. Files under `tests/` are not in
//! `src/`, so this file is not one of the ones it reads.

use std::path::{Path, PathBuf};

/// Field names and inline captures that would put somebody's data in a line at `info`
/// or above.
///
/// Two words are deliberately absent. `names`, because `tls.rs` logs `names =
/// ?domains`, which is the operator's own `TLS_DOMAINS` — configuration, not anybody's
/// data. And `address`, which in this codebase means a socket far more often than a
/// person; `email` is the word that means the personal one, and it is here.
///
/// `items` is absent for the same kind of reason: the plural reads as a count, and a
/// count is exactly what these lines are allowed to carry.
const BANNED: &[&str] = &[
    "name", "item", "list_name", "email", "emails", "token", "code", "line", "contents",
    "statement", "invite", "note", "password", "secret",
];

/// The one place a banned word is allowed, and why.
///
/// `main.rs` prints the claim code at `warn` on purpose, and it is the answer to A2's
/// land grab rather than an oversight: between starting a process and somebody claiming
/// it, anyone who can reach the port would otherwise become the owner, and the person it
/// happens to gets no warning at all. `docs/configuration.md` chose a code written to
/// the log over an environment variable because a self-hoster starts the process and
/// then opens the app, so the log is what they are looking at; it needs no
/// configuration, which suits a packaged install; and it is the only one of the three
/// answers that is safe when the port is already public. It is offered only while nobody
/// owns the server, it is new on every restart, and a code read off last month's log is
/// not a key.
///
/// Moving it behind `contents!` would put it behind `LOG_LEVEL=debug` — which is exactly
/// the configuration step that design exists to avoid.
const ALLOWED: &[(&str, &str)] = &[("server/src/main.rs", "code")];

/// The crates whose log sites are the server's. `embedded` and `quickadd-ffi` are
/// absent because neither logs.
const SCANNED: &[&str] = &["domain", "parsing", "observability", "api", "web", "server"];

#[test]
fn no_log_site_above_debug_carries_contents() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace is this crate's parent")
        .to_path_buf();

    let mut offences = Vec::new();
    let mut sites = 0;

    for crate_name in SCANNED {
        for file in rust_files(&root.join(crate_name).join("src")) {
            let relative = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&file).expect("reading a source file");

            for invocation in log_sites(&source) {
                sites += 1;
                for word in suspicious(&invocation) {
                    if !ALLOWED.contains(&(relative.as_str(), word)) {
                        offences.push(format!("{relative}: `{word}` in {invocation}"));
                    }
                }
            }
        }
    }

    // A scanner that found nothing because it walked the wrong directory would pass
    // silently and for ever, which is the failure mode of every test shaped like this
    // one.
    assert!(sites > 30, "only {sites} log sites were found; the scan is broken");

    assert!(
        offences.is_empty(),
        "these log sites are at info or above and carry something a person typed. \
         Use observability::contents! instead, which cannot emit above debug:\n{}",
        offences.join("\n")
    );
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }

    found
}

/// The text inside every `info!`, `warn!` or `error!` invocation.
///
/// Naive about parentheses inside string literals, and deliberately so: an unbalanced
/// one makes a site read *longer* than it is, which errs towards flagging rather than
/// towards missing.
fn log_sites(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut sites = Vec::new();

    for macro_name in ["info!(", "warn!(", "error!("] {
        let mut from = 0;
        while let Some(at) = source[from..].find(macro_name) {
            let start = from + at;
            let opens = start + macro_name.len();
            from = opens;

            // `slow_warn!(` is not this macro. Only `tracing::` or a non-identifier
            // character may come immediately before the name.
            if source[..start].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }

            let mut depth = 1usize;
            let mut end = opens;
            while end < bytes.len() && depth > 0 {
                match bytes[end] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                end += 1;
            }

            sites.push(source[opens..end.saturating_sub(1)].to_string());
        }
    }

    sites
}

/// The banned words this invocation uses as a field name or an inline capture.
fn suspicious(invocation: &str) -> Vec<&'static str> {
    BANNED
        .iter()
        .copied()
        .filter(|banned| as_a_field(invocation, banned) || as_a_capture(invocation, banned))
        .collect()
}

/// `name = something` — but not `list_name = …` when looking for `name`, and not
/// `name == …`.
fn as_a_field(invocation: &str, banned: &str) -> bool {
    let mut at = 0;
    while let Some(hit) = invocation[at..].find(banned) {
        let start = at + hit;
        let end = start + banned.len();
        at = end;

        let part_of_a_longer_word = invocation[..start]
            .ends_with(|c: char| c.is_alphanumeric() || c == '_')
            || invocation[end..].starts_with(|c: char| c.is_alphanumeric() || c == '_');
        if part_of_a_longer_word {
            continue;
        }

        let rest = invocation[end..].trim_start();
        if rest.starts_with('=') && !rest.starts_with("==") {
            return true;
        }
    }

    false
}

/// `"… {name} …"`. The shape that hides best, because there is no field to notice when
/// reading the line.
fn as_a_capture(invocation: &str, banned: &str) -> bool {
    let mut at = 0;
    while let Some(open) = invocation[at..].find('{') {
        let start = at + open + 1;
        let Some(close) = invocation[start..].find('}') else {
            return false;
        };
        let inside = &invocation[start..start + close];
        at = start + close;

        // `{name:?}` and `{name}` alike; the format specification is not part of the
        // name.
        if inside.split(':').next().unwrap_or("").trim() == banned {
            return true;
        }
    }

    false
}
