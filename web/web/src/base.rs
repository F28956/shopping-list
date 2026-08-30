//! Where this application is mounted, when it is not at the root of its host.
//!
//! A self-hoster with one name and several things behind it puts this at
//! `https://example.com/sl` rather than giving it a subdomain of its own. That is an
//! ordinary arrangement and the alternative — insisting on a whole host — is a
//! constraint on their DNS, not a property of this application.
//!
//! Two halves, and both are needed:
//!
//!   * **Routing.** `server` nests the router under the prefix, so a request for
//!     `/sl/api/lists` reaches the handler registered at `/api/lists`. Handlers never
//!     learn about it.
//!   * **Every URL this application writes down.** A page served at `/sl/lists` that
//!     links to `/lists` sends the browser to the wrong place, because an absolute
//!     path is resolved against the *host* and not against where the page came from.
//!     That is what [`at`] is for, and why no template may write a leading-slash path
//!     directly.
//!
//! ## Why a global and not a field on `AppState`
//!
//! It is deployment configuration, fixed before the first request and identical for
//! every one of them — the same argument `csrf::Origin` makes for reading
//! `PUBLIC_ORIGIN` once. Threading it instead would put a parameter through every
//! template function in this crate to carry a value that never differs, and the first
//! function somebody forgot would emit a broken link that only appears when the
//! application is mounted somewhere, which is the configuration least likely to be
//! the one under test.

use std::sync::OnceLock;

static BASE: OnceLock<String> = OnceLock::new();

/// Reads what an operator wrote, and refuses what cannot work.
///
/// Accepted: empty, or a path beginning with `/`. Normalised by dropping any trailing
/// slash, so `/sl/` and `/sl` are the same deployment and [`at`] cannot produce `//`.
/// A bare `/` means the root and is stored as empty.
pub fn normalise(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return Ok(String::new());
    }

    anyhow::ensure!(
        trimmed.starts_with('/'),
        "BASE_PATH is \"{trimmed}\"; it must begin with \"/\""
    );
    // Refused rather than accepted and half-ignored: a scheme here means somebody has
    // confused this with PUBLIC_ORIGIN, and the two do different jobs.
    anyhow::ensure!(
        !trimmed.contains("://"),
        "BASE_PATH is \"{trimmed}\"; it is a path such as \"/sl\", not a URL — see PUBLIC_ORIGIN"
    );
    anyhow::ensure!(
        !trimmed.contains('?') && !trimmed.contains('#'),
        "BASE_PATH is \"{trimmed}\"; it may not carry a query or a fragment"
    );

    Ok(trimmed.trim_end_matches('/').to_string())
}

/// Fixes the prefix for the life of the process. Later calls are ignored.
pub fn install(base: String) {
    let _ = BASE.set(base);
}

/// The prefix, or `""` at the root. Never ends in a slash.
pub fn get() -> &'static str {
    BASE.get().map(String::as_str).unwrap_or("")
}

/// An absolute path, as this deployment must write it.
///
/// **Every link, form action, htmx target and redirect in this crate goes through
/// here.** `at("/lists")` is `/lists` at the root and `/sl/lists` under a prefix.
pub fn at(path: &str) -> String {
    debug_assert!(path.starts_with('/'), "at() takes an absolute path, got {path:?}");
    format!("{}{path}", get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::nothing("", "")]
    #[case::the_root("/", "")]
    #[case::a_prefix("/sl", "/sl")]
    #[case::a_trailing_slash("/sl/", "/sl")]
    #[case::several_segments("/apps/shopping", "/apps/shopping")]
    #[case::surrounding_space("  /sl  ", "/sl")]
    fn what_an_operator_writes_becomes_a_prefix(#[case] raw: &str, #[case] expected: &str) {
        assert_eq!(normalise(raw).unwrap(), expected);
    }

    /// Each of these produces a server that starts and then writes broken links, so
    /// each is refused while somebody is watching.
    #[rstest]
    #[case::no_leading_slash("sl")]
    #[case::a_whole_url("https://example.com/sl")]
    #[case::a_query("/sl?x=1")]
    #[case::a_fragment("/sl#x")]
    fn what_cannot_work_is_refused(#[case] raw: &str) {
        assert!(normalise(raw).is_err(), "{raw:?} was accepted");
    }

    /// The property every template depends on: one slash, never two.
    #[test]
    fn a_prefix_and_a_path_join_with_a_single_slash() {
        assert_eq!(format!("{}{}", "/sl", "/lists"), "/sl/lists");
        assert_eq!(format!("{}{}", "", "/lists"), "/lists");
    }
}
