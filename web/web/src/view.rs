//! The shared shell. One place that knows what a page looks like, so the handlers
//! only decide what is on it.

use maud::{DOCTYPE, Markup, html};

pub fn page(title: &str, who: Option<&str>, inner: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · Shopping list" }
                // Served rather than inlined so the Content-Security-Policy can say
                // `self` and nothing else — an inline block would need
                // `unsafe-inline`, which is most of what a CSP is for.
                link rel="stylesheet" href="/static/app.css";
                // Vendored, not from a CDN — see assets.rs. `defer` because nothing on
                // the page needs them before the HTML is parsed.
                script src="/static/htmx.js" defer {}
                script src="/static/app.js" defer {}
            }
            body {
                header {
                    h1 { a href="/lists" { "Shopping list" } }
                    @if let Some(name) = who {
                        span class="who" { (name) " · " a href="/auth/logout" { "sign out" } }
                    }
                }
                @if who.is_some() {
                    nav { a href="/lists" { "Lists" } a href="/notes" { "Notes" } }
                }
                (inner)
            }
        }
    }
}

/// The signed-out page. Deliberately the only thing on it.
pub fn sign_in() -> Markup {
    page(
        "Sign in",
        None,
        html! {
            p class="empty" { "Keep your shopping lists in one place." }
            p style="text-align:center" {
                a href="/auth/login" { button class="primary" { "Sign in with Google" } }
            }
        },
    )
}
