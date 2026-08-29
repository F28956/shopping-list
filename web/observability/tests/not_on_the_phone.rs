//! The one thing this crate must never do: reach a phone.
//!
//! `embedded` is compiled into the iOS and Android apps — the same `domain` code the
//! self-hosted server runs, on the device, so a standalone phone and a server behave
//! identically. Binary size and startup are load-bearing there in a way they are not on
//! a server, and an OTLP exporter, a Prometheus registry and a protobuf codec are none
//! of them things a shopping list on a handset should carry.
//!
//! The arrow therefore points one way: `observability` may depend on `domain`, and
//! `domain` may not depend on `observability`. That is easy to state and easy to
//! violate with one line of `cargo add` in the wrong directory, because the result
//! compiles, passes every test, and is only visible in a binary somebody measures
//! months later.
//!
//! Read from the manifests rather than from `cargo tree`, so this is a fast test with
//! no subprocess — and manifests are where the mistake is actually made.

use std::path::Path;

/// Crates a phone links, and which must stay free of any of this.
const ON_THE_PHONE: &[&str] = &["embedded", "domain", "parsing", "quickadd-ffi"];

/// Things whose presence would mean the mistake has been made.
const FORBIDDEN: &[&str] = &["observability", "opentelemetry", "prometheus", "prost", "metrics"];

#[test]
fn metrics_stay_out_of_the_phone() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace is this crate's parent");

    let mut offences = Vec::new();

    for crate_name in ON_THE_PHONE {
        let manifest = workspace.join(crate_name).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("reading {}: {e}", manifest.display()));

        for line in text.lines() {
            // Comments in these manifests talk about exactly this rule, so they are
            // skipped — otherwise the file explaining the rule would break it.
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }

            let Some((left, _)) = line.split_once('=') else {
                continue;
            };
            let dependency = left.trim().trim_matches('"');

            if FORBIDDEN.contains(&dependency) {
                offences.push(format!("{crate_name}/Cargo.toml depends on {dependency}"));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "these are compiled into phones and must not carry metrics or an exporter. \
         Record the outcome in `api` or `web` instead, which no handset links:\n{}",
        offences.join("\n")
    );
}
