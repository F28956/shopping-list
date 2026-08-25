//! What is on one list.

use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::{Redirect, Response};
use domain::models::invite::Token;
use domain::models::item::{self, Amount, Name};
use domain::models::list::{ListMember, Role};
use domain::models::{Direction, OrderBy, Paging, list, tag, unit};
use domain::service::{Actor, items, lists, tags, units};
use maud::{Markup, html};
use tower_sessions::Session;

use crate::auth;
use crate::error::AppError;
use crate::htmx::swap_or_redirect;
use crate::state::AppState;
use crate::view;

/// One line, read as a person means it — see [`domain::quick_add`].
#[derive(serde::Deserialize)]
pub struct QuickAddForm {
    pub line: String,
}

#[derive(serde::Deserialize)]
pub struct NewItem {
    pub name: String,
    pub amount: Option<f64>,
    /// Empty string when the picker is left on "unit", so it cannot be `Option<i64>`
    /// directly — an empty field is not a missing field to a browser.
    pub unit_id: Option<String>,
}

fn everything() -> Paging {
    domain::service::everything()
}

/// Exposed so a test can assert the page has not drifted from the shared ceiling.
#[cfg(test)]
pub fn page_cap() -> i64 {
    everything().size
}

/// Everything the item rows need, gathered once.
struct Board {
    items: Vec<item::Item>,
    /// What this person may do here, which decides what the page offers them.
    role: Role,
    /// Everyone else on the list, so it is obvious who can see your shopping.
    members: Vec<ListMember>,
    /// How many are on this list in total, and whether the page holds them all.
    total: i64,
    truncated: bool,
    /// What this person has bought before, for the quick-add suggestions.
    suggestions: Vec<item::Name>,
    unit_names: std::collections::HashMap<i64, String>,
    tags_by_item: std::collections::HashMap<i64, Vec<tag::Tag>>,
    all_tags: Vec<tag::Tag>,
}

async fn board(s: &AppState, actor: &Actor, list_id: list::Id) -> Result<Board, AppError> {
    let page = items::for_list(
        &s.ctx,
        actor,
        list_id,
        everything(),
        // Outstanding first, then the ones already ticked off.
        OrderBy {
            field: item::Field::DoneAt,
            direction: Direction::Ascending,
        },
    )
    .await?;

    Ok(Board {
        total: page.total,
        truncated: page.has_more,
        items: page.items,
        role: lists::role(&s.ctx, actor, list_id).await?,
        members: lists::members(&s.ctx, actor, list_id).await?,
        suggestions: items::suggestions(&s.ctx, actor, list_id, 100).await?,
        unit_names: unit_lookup(s, actor).await?,
        // One query for the whole page rather than one per item.
        tags_by_item: tags::for_list(&s.ctx, actor, list_id).await?,
        all_tags: tags::list(
            &s.ctx,
            actor,
            everything(),
            OrderBy {
                field: tag::Field::Name,
                direction: Direction::Ascending,
            },
        )
        .await?
        .items,
    })
}

/// The part of the page that changes. Every edit re-renders exactly this, so there is
/// one description of what an item looks like rather than one per operation.
///
/// One row per item, and it reads as a shopping list: tick box, what it is, what it
/// is tagged, how much. Everything that *changes* an item — rename, amount, unit,
/// tags, delete — lives behind the one disclosure at the end, because a list you are
/// reading in a shop should not be covered in buttons you are not pressing.
///
/// `open` is the item whose panel should come back expanded. Acting inside the panel
/// swaps this whole element, so anything that should stay open has to say so — and
/// anything that finishes a job, like saving an edit, deliberately does not.
fn fragment(list_id: list::Id, b: &Board, open: Option<i64>) -> Markup {
    let base = format!("/lists/{}", list_id.0);
    html! {
        div id="items" {
            @if b.items.is_empty() {
                p class="empty" { "Nothing on this list yet." }
            } @else {
                // A page that quietly shows a prefix is worse than one that admits to
                // it: the missing items look deleted rather than merely elsewhere.
                @if b.truncated {
                    p class="truncated" {
                        "Showing " (b.items.len()) " of " (b.total)
                        ". This list is long enough to be worth splitting."
                    }
                }
                @let groups = group_by_category(b);
                @for (heading, items) in &groups {
                    section class="group" {
                        h3 class="group-heading" { (heading) }
                        ul class="rows" {
                            @for i in items { (item_row(list_id, b, i, open)) }
                        }
                    }
                }

                @let done = b.items.iter().filter(|i| i.done_at.is_some()).count();
                @if done > 0 {
                    // Ticked items are collected rather than left in place: a list you
                    // are working through should show what is left, not what is behind
                    // you. Still one click away, because "did I get that?" is a real
                    // question in a shop.
                    details class="done-drawer" {
                        summary { "✓ " (done) " done" }
                        ul class="rows" {
                            @for i in b.items.iter().filter(|i| i.done_at.is_some()) {
                                (item_row(list_id, b, i, open))
                            }
                        }
                        form class="inline" method="post" action={ (base) "/clear-done" }
                             hx-post={ (base) "/clear-done" }
                             hx-target="#items" hx-swap="outerHTML"
                             hx-confirm={ "Remove all " (done) " ticked items?" } {
                            button class="danger" { "Clear done" }
                        }
                    }
                }
            }
        }
    }
}

/// One item: what it is, what it is tagged, how much — and, behind the toggle,
/// everything that changes it.
fn item_row(list_id: list::Id, b: &Board, i: &item::Item, open: Option<i64>) -> Markup {
    let base = format!("/lists/{}", list_id.0);
    let item = format!("{base}/items/{}", i.id.0);
    let on_item = b.tags_by_item.get(&i.id.0);

    html! {
        li class=@if i.done_at.is_some() { "item done" } @else { "item" } {
            // The switch sits first so CSS can hide the row and show the editor in its
            // place: an item being edited is one thing in one position, not a row with
            // a drawer under it.
            input type="checkbox" class="panel-switch" hidden
                  id=(format!("panel-{}", i.id.0))
                  checked[open == Some(i.id.0)];

            div class="view" {
                form class="inline" method="post" action={ (item) "/toggle" }
                     hx-post={ (item) "/toggle" }
                     hx-target="#items" hx-swap="outerHTML" {
                    button class="tick" title="Tick off" {
                        @if i.done_at.is_some() { "☑" } @else { "☐" }
                    }
                }

                span class="grow" {
                    (i.name.0)
                    // Tags are shown, not operated, out here. Inside a category group
                    // the heading already says the first one, so only the extras earn
                    // their space.
                    @for t in on_item.into_iter().flatten().skip(1) {
                        span class="chip" {
                            @if let Some(e) = &t.emoji { (e.0) " " }
                            (t.name.0)
                        }
                    }
                }

                @if let Some(measure) = measure(i, &b.unit_names) {
                    span class="amount" { (measure) }
                }

                label class="panel-toggle" for=(format!("panel-{}", i.id.0))
                      title="Edit" { "⋯" }
            }

            div class="panel-body" {
                form class="add" method="post" action={ (item) "/edit" }
                     hx-post={ (item) "/edit" }
                     hx-target="#items" hx-swap="outerHTML" {
                    input type="text" name="name" value=(i.name.0)
                          required maxlength="128" aria-label="Item name";
                    input type="number" name="amount" value=(trim_amount(i.amount))
                          min="0" step="any" style="width:5rem" aria-label="Amount";
                    select name="unit_id" aria-label="Unit" {
                        option value="" selected[i.unit_id.is_none()] { "unit" }
                        @for (uid, uname) in &unit_names_sorted(&b.unit_names) {
                            option value=(uid) selected[i.unit_id.map(|u| u.0) == Some(*uid)] {
                                (uname)
                            }
                        }
                    }
                    button class="primary" { "Save" }
                    // A label, not a button: it unticks the switch and so puts the row
                    // back, with no JavaScript needed. app.js additionally drops the
                    // unsaved typing; without it the field simply keeps what was typed.
                    label class="cancel" for=(format!("panel-{}", i.id.0)) { "Cancel" }
                }

                div class="tag-edit" {
                    @for t in on_item.into_iter().flatten() {
                        form class="inline" method="post"
                             action={ (item) "/tags/" (t.id.0) "/delete" }
                             hx-post={ (item) "/tags/" (t.id.0) "/delete" }
                             hx-target="#items" hx-swap="outerHTML" {
                            button class="chip removable" title="Remove tag" {
                                @if let Some(e) = &t.emoji { (e.0) " " }
                                (t.name.0) " ×"
                            }
                        }
                    }
                    // Choosing a tag is the whole action, so `change` fires it. The
                    // confirm button only exists for browsers that cannot post on
                    // their own, which is what <noscript> says precisely.
                    form class="inline" method="post" action={ (item) "/tags" }
                         hx-post={ (item) "/tags" }
                         hx-target="#items" hx-swap="outerHTML"
                         hx-trigger="change" {
                        select class="tag-add" name="tag_id" aria-label="Add a tag" required {
                            option value="" disabled selected { "+ tag" }
                            @for t in &b.all_tags {
                                // only what is not already on it
                                @if !on_item.is_some_and(|ts| ts.iter().any(|x| x.id == t.id)) {
                                    option value=(t.id.0) {
                                        @if let Some(e) = &t.emoji { (e.0) " " }
                                        (t.name.0)
                                    }
                                }
                            }
                        }
                        noscript { button { "Add" } }
                    }
                }

                form class="inline" method="post" action={ (item) "/delete" }
                     hx-post={ (item) "/delete" }
                     hx-target="#items" hx-swap="outerHTML"
                     hx-confirm={ "Remove " (i.name.0) "?" } {
                    button class="danger" { "Remove item" }
                }
            }
        }
    }
}

/// How much of it, or nothing at all.
///
/// One of something unmeasured is the default and the commonest case, so printing
/// "1" on most rows is noise dressed as information.
fn measure(i: &item::Item, units: &std::collections::HashMap<i64, String>) -> Option<String> {
    let unit = i.unit_id.and_then(|u| units.get(&u.0));
    match (i.amount.0, unit) {
        (1.0, None) => None,
        (a, None) => Some(trim_amount(item::Amount(a))),
        (a, Some(u)) => Some(format!("{} {u}", trim_amount(item::Amount(a)))),
    }
}

/// The outstanding items, under their category heading, in the order the shop is laid
/// out — see the `sort_order` migration for why that order and not the alphabet.
///
/// An item with several tags falls under its first, which `Tag::for_list` has already
/// ordered by `sort_order`; an untagged one falls under "Other", last.
fn group_by_category(b: &Board) -> Vec<(String, Vec<&item::Item>)> {
    let mut groups: Vec<(i64, String, Vec<&item::Item>)> = Vec::new();

    for i in b.items.iter().filter(|i| i.done_at.is_none()) {
        let primary = b.tags_by_item.get(&i.id.0).and_then(|ts| ts.first());
        let (order, heading) = match primary {
            Some(t) => (
                t.sort_order.0,
                match &t.emoji {
                    Some(e) => format!("{} {}", e.0, t.name.0),
                    None => t.name.0.clone(),
                },
            ),
            // Untagged sorts last, whatever the tags are numbered.
            None => (i64::MAX, "Other".to_string()),
        };

        match groups.iter_mut().find(|(_, h, _)| *h == heading) {
            Some((_, _, items)) => items.push(i),
            None => groups.push((order, heading, vec![i])),
        }
    }

    groups.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    groups.into_iter().map(|(_, h, i)| (h, i)).collect()
}

pub async fn show(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();

    // Ownership is checked here, once, by the service. A list that is not theirs is
    // NotFound, which the browser sees as a 404 page rather than a hint.
    let list = lists::get(&s.ctx, &actor, list::Id(id)).await?;
    let b = board(&s, &actor, list.id).await?;

    Ok(view::page(
        &list.name.0,
        Some(&crate::pages::who(&user)),
        html! {
            p { a href="/" { "← all lists" } }
            h2 style="font-size:1.1rem;margin:.5rem 0 1rem" { (list.name.0) }

            (fragment(list.id, &b, None))

            (share_panel(&list, &b))

            // One field. "2 kg apples" is parsed into the three the model wants —
            // see domain::quick_add — and the editor is there for anything it reads
            // wrongly. The datalist is this person's own history, because the thing
            // people actually complain about is typing "Milk" again every week.
            @if b.role >= Role::Editor {
            form class="add" method="post" action={ "/lists/" (list.id.0) "/items" }
                 hx-post={ "/lists/" (list.id.0) "/items" }
                 hx-target="#items" hx-swap="outerHTML" {
                input type="text" name="line" placeholder="Add an item — try 2 kg apples"
                      required maxlength="200" autocomplete="off" list="item-history";
                button class="primary" { "Add" }
            }

            datalist id="item-history" {
                @for name in &b.suggestions { option value=(name.0) {} }
            }
            }
        },
    ))
}

/// `2` rather than `2.0`, but `1.5` stays `1.5`.
fn trim_amount(a: Amount) -> String {
    if a.0.fract() == 0.0 {
        format!("{}", a.0 as i64)
    } else {
        format!("{}", a.0)
    }
}

async fn unit_lookup(
    s: &AppState,
    actor: &domain::service::Actor,
) -> Result<std::collections::HashMap<i64, String>, AppError> {
    let units = units::list(
        &s.ctx,
        actor,
        everything(),
        OrderBy {
            field: unit::Field::Name,
            direction: Direction::Ascending,
        },
    )
    .await?;
    Ok(units
        .items
        .into_iter()
        .map(|u| (u.id.0, u.name.0))
        .collect())
}

fn unit_names_sorted(units: &std::collections::HashMap<i64, String>) -> Vec<(i64, String)> {
    let mut v: Vec<_> = units.iter().map(|(k, x)| (*k, x.clone())).collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

pub async fn create(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<QuickAddForm>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;

    // Parsing, the remembered unit and the remembered category all live in the
    // service, so the browser and the API cannot disagree about what a line means.
    items::quick_add(&s.ctx, &actor, list::Id(id), &form.line).await?;

    swap(&s, &actor, &headers, list::Id(id), None).await
}

/// Clears everything already ticked off the list.
pub async fn clear_done(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    items::clear_done(&s.ctx, &actor, list::Id(id)).await?;
    swap(&s, &actor, &headers, list::Id(id), None).await
}

/// One button that flips whichever way the item currently is — a browser form cannot
/// send a PUT, and asking the page to remember which state it was in invites the two
/// to disagree.
pub async fn toggle(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;

    let item = items::get(&s.ctx, &actor, item::Id(item_id)).await?;
    items::set_done(&s.ctx, &actor, item.id, item.done_at.is_none()).await?;

    swap(&s, &actor, &headers, list::Id(list_id), None).await
}

pub async fn delete(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    items::delete(&s.ctx, &actor, item::Id(item_id)).await?;
    swap(&s, &actor, &headers, list::Id(list_id), None).await
}

#[derive(serde::Deserialize)]
pub struct TagChoice {
    pub tag_id: i64,
}

/// Tagging is an edit to the item, so the route sits under the item and the service
/// checks the item's list — the tag itself grants nothing.
pub async fn attach_tag(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
    headers: HeaderMap,
    Form(form): Form<TagChoice>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    tags::attach(&s.ctx, &actor, item::Id(item_id), tag::Id(form.tag_id)).await?;
    swap(&s, &actor, &headers, list::Id(list_id), Some(item_id)).await
}

pub async fn detach_tag(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id, tag_id)): Path<(i64, i64, i64)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    tags::detach(&s.ctx, &actor, item::Id(item_id), tag::Id(tag_id)).await?;
    swap(&s, &actor, &headers, list::Id(list_id), Some(item_id)).await
}

/// Edits what a person typed: name, amount, unit. Not the list it is on -- moving an
/// item between lists would need the destination checked too, and is its own
/// operation rather than a field on this form.
pub async fn edit(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
    headers: HeaderMap,
    Form(form): Form<NewItem>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;

    let unit_id = form
        .unit_id
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<i64>().ok())
        .map(unit::Id);

    items::update(
        &s.ctx,
        &actor,
        item::Id(item_id),
        Name(form.name),
        Amount(form.amount.unwrap_or(1.0)),
        unit_id,
    )
    .await?;

    // Saving finishes the job, so the panel closes behind it. Tagging does not:
    // people add two or three at a time, and reopening the panel for each one would
    // mean four clicks to do what should take two.
    swap(&s, &actor, &headers, list::Id(list_id), None).await
}

/// Re-renders the item board for htmx, or sends a browser back to the page.
/// Re-renders the item board for htmx, or sends a browser back to the page.
///
/// `open` decides whether the panel comes back expanded. Acting inside it swaps the
/// whole board, so the markup has to say so — but only where staying open is what the
/// person wants. Tagging keeps it; saving an edit closes it, because the job is done.
async fn swap(
    s: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    list_id: list::Id,
    open: Option<i64>,
) -> Result<Response, AppError> {
    let to = format!("/lists/{}", list_id.0);
    if crate::htmx::is_htmx(headers) {
        let b = board(s, actor, list_id).await?;
        Ok(swap_or_redirect(headers, fragment(list_id, &b, open), &to))
    } else {
        Ok(swap_or_redirect(headers, maud::html! {}, &to))
    }
}

/// Who else is on this list, and — for the owner — how to add somebody.
///
/// Shown to everyone, because knowing who can see your shopping is part of knowing
/// what sharing means. Only the owner is offered the controls.
fn share_panel(list: &list::List, b: &Board) -> Markup {
    let base = format!("/lists/{}", list.id.0);
    html! {
        section class="share" {
            h3 class="group-heading" {
                @if b.members.is_empty() { "Not shared" } @else { "Shared with" }
            }

            @if !b.members.is_empty() {
                ul class="rows" {
                    @for m in &b.members {
                        li class="item" {
                            span class="grow" { "Someone" }
                            span class="amount" { (role_name(m.role)) }
                            @if b.role >= Role::Owner {
                                form class="inline" method="post"
                                     action={ (base) "/members/" (m.user_id.0) "/remove" }
                                     hx-post={ (base) "/members/" (m.user_id.0) "/remove" }
                                     hx-target="body" hx-swap="outerHTML" {
                                    button class="quiet" title="Remove" { "×" }
                                }
                            }
                        }
                    }
                }
            }

            @if b.role >= Role::Owner {
                form class="add" method="post" action={ (base) "/invites" } {
                    select name="role" aria-label="What they may do" {
                        option value="editor" { "can add and tick off" }
                        option value="viewer" { "can only look" }
                    }
                    button { "Create a link" }
                }
            } @else {
                // A member should be able to get out without asking the owner.
                form class="inline" method="post" action={ (base) "/leave" } {
                    button class="quiet" { "Leave this list" }
                }
            }
        }
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Editor => "can edit",
        Role::Viewer => "can look",
    }
}

/// Creates an invitation and shows the link once.
///
/// Once, because only its hash is stored — an owner who loses it makes another. The
/// page says so rather than letting somebody discover it.
pub async fn invite(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<InviteForm>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();
    let list = lists::get(&s.ctx, &actor, list::Id(id)).await?;

    let token = lists::invite(&s.ctx, &actor, list::Id(id), form.role).await?;
    let origin =
        std::env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let link = format!("{origin}/join/{}", token.0);

    Ok(view::page(
        &list.name.0,
        Some(&crate::pages::who(&user)),
        html! {
            p { a href={ "/lists/" (list.id.0) } { "← back to " (list.name.0) } }
            h2 style="font-size:1.1rem;margin:.5rem 0" { "Invitation link" }
            p { "Send this to whoever should join. It works once per person and stops "
                "working after a week." }
            p class="token" { (link) }
            p class="truncated" {
                "This is the only time it is shown — nothing stores the link itself, "
                "only enough to recognise it. Lose it and make another."
            }
            form class="inline" method="post" action={ "/lists/" (list.id.0) "/invites/revoke" } {
                button class="danger" { "Cancel all outstanding links" }
            }
        },
    ))
}

#[derive(serde::Deserialize)]
pub struct InviteForm {
    pub role: Role,
}

pub async fn revoke_invites(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::revoke_invites(&s.ctx, &actor, list::Id(id)).await?;
    Ok(Redirect::to(&format!("/lists/{id}")))
}

pub async fn remove_member(
    session: Session,
    State(s): State<AppState>,
    Path((id, who)): Path<(i64, i64)>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::remove_member(&s.ctx, &actor, list::Id(id), domain::models::user::Id(who)).await?;
    Ok(Redirect::to(&format!("/lists/{id}")))
}

/// Leaving is removing yourself, so it is the same operation.
pub async fn leave(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let me = actor.person()?.id;
    lists::remove_member(&s.ctx, &actor, list::Id(id), me).await?;
    Ok(Redirect::to("/"))
}

/// Following an invitation link.
pub async fn join(
    session: Session,
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let list = lists::join(&s.ctx, &actor, &Token(token)).await?;
    Ok(Redirect::to(&format!("/lists/{}", list.id.0)))
}
