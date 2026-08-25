//! The shared shell. One place that knows what a page looks like, so the handlers
//! only decide what is on it.

use maud::{DOCTYPE, Markup, PreEscaped, html};

/// Small enough to inline, and inlining it means no second request and no cache to
/// invalidate. If this grows past a screenful it wants to be a served file.
const STYLE: &str = r#"
:root { color-scheme: light dark; --line: color-mix(in oklab, currentColor 15%, transparent); }
* { box-sizing: border-box; }
body { font: 16px/1.5 system-ui, -apple-system, sans-serif; margin: 0 auto; padding: 1.5rem 1rem 4rem;
       max-width: 34rem; }
h1 { font-size: 1.4rem; margin: 0; }
h1 a { color: inherit; text-decoration: none; }
header { display: flex; justify-content: space-between; align-items: baseline; gap: 1rem;
         border-bottom: 2px solid currentColor; padding-bottom: .6rem; margin-bottom: 1.2rem; }
header .who { font-size: .8rem; opacity: .7; }
nav { display: flex; gap: 1rem; font-size: .85rem; margin-bottom: 1.2rem; }
ul.rows { list-style: none; padding: 0; margin: 0; }
ul.rows li { display: flex; align-items: center; gap: .6rem; padding: .55rem 0;
             border-bottom: 1px solid var(--line); }
ul.rows li .grow { flex: 1; min-width: 0; }
ul.rows a { color: inherit; }
.amount { opacity: .6; font-variant-numeric: tabular-nums; font-size: .85rem; white-space: nowrap; }
.done .grow { opacity: .45; text-decoration: line-through; }
form.inline { display: contents; }
form.add { display: flex; gap: .4rem; margin: 1.2rem 0; flex-wrap: wrap; }
form.add input[type=text] { flex: 1 1 8rem; min-width: 0; }
input, select, button { font: inherit; padding: .4rem .5rem; border-radius: 4px;
                        border: 1px solid var(--line); background: transparent; color: inherit; }
button { cursor: pointer; }
button.primary { border-color: currentColor; }
button.quiet { border: 0; opacity: .5; padding: .2rem .35rem; }
button.quiet:hover { opacity: 1; }
.empty { opacity: .6; padding: 2rem 0; text-align: center; }
"#;

pub fn page(title: &str, who: Option<&str>, inner: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · Shopping list" }
                style { (PreEscaped(STYLE)) }
            }
            body {
                header {
                    h1 { a href="/" { "Shopping list" } }
                    @if let Some(name) = who {
                        span class="who" { (name) " · " a href="/auth/logout" { "sign out" } }
                    }
                }
                @if who.is_some() {
                    nav { a href="/" { "Lists" } a href="/notes" { "Notes" } }
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
