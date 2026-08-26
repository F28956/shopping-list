//! What is on one list.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Form, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Redirect;
use axum::response::Response;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use domain::models::item::{self, Amount, Name};
use domain::models::list::Role;
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
    /// How many are on this list in total, and whether the page holds them all.
    total: i64,
    truncated: bool,
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
        unit_names: unit_lookup(s, actor).await?,
        // One query for the whole page rather than one per item.
        tags_by_item: tags::for_list(&s.ctx, actor, list_id).await?,
        // In this person's order for this list, not the global one: `group_by_category`
        // reads position in this vector, so the whole rule lives in the service.
        all_tags: tags::order_for(&s.ctx, actor, list_id).await?,
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
                        @if b.role >= Role::Editor {
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
                @if b.role >= Role::Editor {
                form class="inline" method="post" action={ (item) "/toggle" }
                     hx-post={ (item) "/toggle" }
                     hx-target="#items" hx-swap="outerHTML" {
                    button class="tick" title="Tick off" {
                        @if i.done_at.is_some() { "☑" } @else { "☐" }
                    }
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

                @if b.role >= Role::Editor {
                    label class="panel-toggle" for=(format!("panel-{}", i.id.0))
                          title="Edit" { "⋯" }
                }
            }

            @if b.role >= Role::Editor {
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
}

/// The unit that means an item is counted rather than measured.
///
/// Stored rather than left NULL, so that `milk` and `1 unit milk` are the same thing
/// and merge; printed as nothing, because it says nothing a number does not.
const UNMEASURED: &str = "unit";

/// How much of it, or nothing at all.
///
/// One of something unmeasured is the default and the commonest case, so printing
/// "1" on most rows is noise dressed as information.
fn measure(i: &item::Item, units: &std::collections::HashMap<i64, String>) -> Option<String> {
    // `unit` is the unit that means "counted, not measured", and it is what an item
    // added without one is given. It says nothing a number does not, so it prints as
    // nothing: six eggs, not "6 unit".
    let unit = i
        .unit_id
        .and_then(|u| units.get(&u.0))
        .filter(|name| name.as_str() != UNMEASURED);

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

    // Where a tag falls for this person on this list: its place in `all_tags`, which
    // the service has already resolved. Position rather than `sort_order`, so a tag
    // somebody has put first leads even though the shop puts it last.
    let placed = |id: tag::Id| -> i64 {
        b.all_tags
            .iter()
            .position(|t| t.id == id)
            .map_or(i64::MAX - 1, |at| at as i64)
    };

    for i in b.items.iter().filter(|i| i.done_at.is_none()) {
        // The item's first tag is whichever of its tags leads in this order.
        let primary = b
            .tags_by_item
            .get(&i.id.0)
            .and_then(|ts| ts.iter().min_by_key(|t| placed(t.id)));
        let (order, heading) = match primary {
            Some(t) => (
                placed(t.id),
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

/// Just the list region, for a client that has been told it is out of date.
///
/// The same fragment the mutating routes return, so a screen refreshed by an event
/// and a screen refreshed by its own edit cannot come out different.
pub async fn fragment_only(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let list = lists::get(&s.ctx, &actor, list::Id(id)).await?;
    let b = board(&s, &actor, list.id).await?;
    Ok(fragment(list.id, &b, None))
}

/// Says when this list changed, so a browser left open stops showing what has gone.
///
/// The browser's own route rather than the API's: `/api` deliberately has no session
/// layer, so a cookie cannot authenticate there, and `EventSource` cannot set an
/// Authorization header. Same notifier underneath, so a phone and a browser hear the
/// same changes.
pub async fn events(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let list_id = list::Id(id);
    lists::get(&s.ctx, &actor, list_id).await?;

    let watching = BroadcastStream::new(s.ctx.changes.watch());
    let stream = watching.filter_map(move |heard| match heard {
        Ok(changed) if changed.list_id == list_id => Some(Ok(Event::default()
            .event("changed")
            .data(changed.list_id.0.to_string()))),
        // Lagging means events were dropped. For a nudge that is the same news as one
        // arriving, and staying quiet would leave the page silently stale.
        Err(_) => Some(Ok(Event::default().event("changed").data(id.to_string()))),
        Ok(_) => None,
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// The options a browser offers under the add field.
fn suggestion_list(names: &[item::Name]) -> Markup {
    html! {
        datalist id="item-history" {
            @for name in names { option value=(name.0) {} }
        }
    }
}

/// The suggestions for what has been typed so far.
///
/// A route rather than filtering in JavaScript: the matching lives in the service,
/// so the browser and the phone offer the same things for the same letters. It costs
/// a request per keystroke-ish -- htmx debounces it -- which is the price of not
/// having a second matcher here to drift from the first.
pub async fn suggestions(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(typed): Query<Typed>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;

    // Nothing typed, nothing offered.
    let Some(query) = typed.line.as_deref().map(str::trim).filter(|q| !q.is_empty()) else {
        return Ok(suggestion_list(&[]));
    };

    // The same pool the API considers. Two different numbers meant the phone could
    // suggest something this page would never offer for the same letters.
    let names = items::suggestions(
        &s.ctx,
        &actor,
        list::Id(id),
        domain::service::PAGE_MAX,
        Some(query),
    )
    .await?;
    Ok(suggestion_list(&names))
}

/// What has been typed into the add field so far.
///
/// Named `line` because that is the input's own name, and htmx sends a field under
/// its name. The API calls the same thing `q`; they are different conventions on
/// different transports, and the matching they both reach is the one in the service.
#[derive(serde::Deserialize)]
pub struct Typed {
    pub line: Option<String>,
}

/// Which tag decides where an item sits on this list.
///
/// Up and down rather than dragging: reordering by drag needs JavaScript, and every
/// other control here works without it. The order is this person's — see
/// [`tags::order_for`] — so nothing on this page changes what anyone else sees.
pub async fn tag_order(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();
    let list = lists::get(&s.ctx, &actor, list::Id(id)).await?;
    let ordered = tags::order_for(&s.ctx, &actor, list.id).await?;

    let base = format!("/lists/{}", list.id.0);
    let last = ordered.len().saturating_sub(1);

    Ok(view::page(
        "Tag order",
        Some(&crate::pages::who(&user)),
        html! {
            p { a href=(base) { "← " (list.name.0) } }
            h2 style="font-size:1.1rem;margin:.5rem 0" { "Tag order" }
            p class="hint" {
                "An item sits under the first of its tags in this order. "
                "This is your order for this list; everyone else keeps theirs."
            }

            form method="post" action={ (base) "/tags/reset" } class="inline" {
                button class="danger" { "Back to shop order" }
            }

            ol class="tag-order" {
                @for (at, tag) in ordered.iter().enumerate() {
                    li {
                        span class="grow" {
                            @if let Some(emoji) = &tag.emoji { (emoji.0) " " }
                            (tag.name.0)
                        }
                        form method="post" action={ (base) "/tags/move" } class="inline" {
                            input type="hidden" name="tag_id" value=(tag.id.0);
                            input type="hidden" name="up" value="true";
                            button disabled[at == 0] title="Up" { "↑" }
                        }
                        form method="post" action={ (base) "/tags/move" } class="inline" {
                            input type="hidden" name="tag_id" value=(tag.id.0);
                            input type="hidden" name="up" value="false";
                            button disabled[at == last] title="Down" { "↓" }
                        }
                    }
                }
            }
        },
    ))
}

/// One step up or down. The whole order is written back, because "one step" only
/// means anything against the order that was on screen when the button was pressed.
#[derive(serde::Deserialize)]
pub struct MoveTag {
    pub tag_id: i64,
    pub up: bool,
}

pub async fn move_tag(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<MoveTag>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let list_id = list::Id(id);

    let mut ordered: Vec<tag::Id> = tags::order_for(&s.ctx, &actor, list_id)
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();

    if let Some(at) = ordered.iter().position(|t| t.0 == form.tag_id) {
        let to = if form.up { at.checked_sub(1) } else { at.checked_add(1) };
        // At either end there is nowhere to go, and the button is disabled anyway;
        // a request that arrives regardless is a no-op rather than a failure.
        if let Some(to) = to.filter(|to| *to < ordered.len()) {
            ordered.swap(at, to);
            tags::set_order(&s.ctx, &actor, list_id, &ordered).await?;
        }
    }

    Ok(Redirect::to(&format!("/lists/{id}/tags")))
}

pub async fn reset_tag_order(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    tags::set_order(&s.ctx, &actor, list::Id(id), &[]).await?;
    Ok(Redirect::to(&format!("/lists/{id}/tags")))
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

    // Remembered only after the access check, so a list somebody cannot open is never
    // the one they get sent back to.
    session.insert(auth::LAST_LIST, list.id.0).await?;
    let b = board(&s, &actor, list.id).await?;

    Ok(view::page(
        &list.name.0,
        Some(&crate::pages::who(&user)),
        html! {
            p {
                a href="/lists" { "← all lists" }
                " · "
                a href={ "/lists/" (list.id.0) "/tags" } { "tag order" }
            }
            h2 style="font-size:1.1rem;margin:.5rem 0 1rem" { (list.name.0) }

            (fragment(list.id, &b, None))

            // Outside `#items` on purpose: this is what app.js reads to open the
            // event stream, and it must not be replaced by the swaps it triggers.
            div id="live" hidden
                data-events={ "/lists/" (list.id.0) "/events" }
                data-items={ "/lists/" (list.id.0) "/items" } {}

            // One field. "2 kg apples" is parsed into the three the model wants —
            // see domain::quick_add — and the editor is there for anything it reads
            // wrongly. The datalist is this person's own history, because the thing
            // people actually complain about is typing "Milk" again every week.
            @if b.role >= Role::Editor {
            form class="add" method="post" action={ "/lists/" (list.id.0) "/items" }
                 hx-post={ "/lists/" (list.id.0) "/items" }
                 hx-target="#items" hx-swap="outerHTML" {
                input type="text" name="line" placeholder="Add an item — try 2 kg apples"
                      required maxlength="200" autocomplete="off" list="item-history"
                      hx-get={ "/lists/" (list.id.0) "/suggestions" }
                      hx-trigger="keyup changed delay:150ms"
                      hx-target="#item-history" hx-swap="outerHTML";
                button class="primary" { "Add" }
            }

            // Empty until something is typed. A datalist the browser has been given
            // in full is shown in full the moment the field is focused, which is a
            // second list on top of the one you came to read.
            (suggestion_list(&[]))
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
    items::quick_add(&s.ctx, &actor, list::Id(id), None, &form.line).await?;

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
    items::clear_done(&s.ctx, &actor, list::Id(id), None).await?;
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
