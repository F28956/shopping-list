//! What is on one list.

use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use domain::models::item::{self, Amount, Name};
use domain::models::{Direction, OrderBy, Paging, list, tag, unit};
use domain::service::{Actor, items, lists, tags, units};
use maud::{Markup, html};
use tower_sessions::Session;

use crate::auth;
use crate::error::AppError;
use crate::htmx::swap_or_redirect;
use crate::state::AppState;
use crate::view;

#[derive(serde::Deserialize)]
pub struct NewItem {
    pub name: String,
    pub amount: Option<f64>,
    /// Empty string when the picker is left on "unit", so it cannot be `Option<i64>`
    /// directly — an empty field is not a missing field to a browser.
    pub unit_id: Option<String>,
}

fn everything() -> Paging {
    Paging {
        number: 1,
        size: 200,
    }
}

/// Everything the item rows need, gathered once.
struct Board {
    items: Vec<item::Item>,
    unit_names: std::collections::HashMap<i64, String>,
    tags_by_item: std::collections::HashMap<i64, Vec<tag::Tag>>,
    all_tags: Vec<tag::Tag>,
}

async fn board(s: &AppState, actor: &Actor, list_id: list::Id) -> Result<Board, AppError> {
    Ok(Board {
        items: items::for_list(
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
        .await?
        .items,
        unit_names: unit_lookup(s, actor).await?,
        // One query for the whole page rather than one per item.
        tags_by_item: tags::on_list(&s.ctx, actor, list_id).await?,
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
/// swaps this whole element, which would otherwise snap it shut mid-edit.
fn fragment(list_id: list::Id, b: &Board, open: Option<i64>) -> Markup {
    let base = format!("/lists/{}", list_id.0);
    html! {
        div id="items" {
            @if b.items.is_empty() {
                p class="empty" { "Nothing on this list yet." }
            } @else {
                ul class="rows" {
                    @for i in &b.items {
                        @let on_item = b.tags_by_item.get(&i.id.0);
                        @let item = format!("{base}/items/{}", i.id.0);
                        li class=@if i.done_at.is_some() { "item done" } @else { "item" } {
                            form class="inline" method="post" action={ (item) "/toggle" }
                                 hx-post={ (item) "/toggle" }
                                 hx-target="#items" hx-swap="outerHTML" {
                                button class="tick" title="Tick off" {
                                    @if i.done_at.is_some() { "☑" } @else { "☐" }
                                }
                            }

                            span class="grow" {
                                (i.name.0)
                                // Tags are shown, not operated, out here: what an item
                                // is tagged is worth knowing at a glance; changing it
                                // is not worth a control on every row.
                                @for t in on_item.into_iter().flatten() {
                                    span class="chip" {
                                        @if let Some(e) = &t.emoji { (e.0) " " }
                                        (t.name.0)
                                    }
                                }
                            }

                            span class="amount" {
                                (trim_amount(i.amount))
                                @if let Some(u) = i.unit_id.and_then(|u| b.unit_names.get(&u.0)) {
                                    " " (u)
                                }
                            }

                            // A checkbox rather than <details>: the panel has to be a
                            // full-width sibling of the row, and a <details> can only
                            // hold its content *inside* itself — which put the panel
                            // in a narrow right-aligned box and pushed the toggle out
                            // of reach. The checkbox stays in the row and the panel
                            // sits beside it, shown by a CSS sibling selector.
                            input type="checkbox" class="panel-switch" hidden
                                  id=(format!("panel-{}", i.id.0))
                                  checked[open == Some(i.id.0)];
                            label class="panel-toggle" for=(format!("panel-{}", i.id.0))
                                  title="Edit" { "⋯" }
                            div class="panel-body" {
                                    form class="add" method="post" action={ (item) "/edit" }
                                         hx-post={ (item) "/edit" }
                                         hx-target="#items" hx-swap="outerHTML" {
                                        input type="text" name="name" value=(i.name.0)
                                              required maxlength="128" aria-label="Item name";
                                        input type="number" name="amount"
                                              value=(trim_amount(i.amount))
                                              min="0" step="any" style="width:5rem"
                                              aria-label="Amount";
                                        select name="unit_id" aria-label="Unit" {
                                            option value="" selected[i.unit_id.is_none()] { "unit" }
                                            @for (uid, uname) in &unit_names_sorted(&b.unit_names) {
                                                option value=(uid)
                                                       selected[i.unit_id.map(|u| u.0) == Some(*uid)] {
                                                    (uname)
                                                }
                                            }
                                        }
                                        button { "Save" }
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
                                        form class="inline" method="post" action={ (item) "/tags" }
                                             hx-post={ (item) "/tags" }
                                             hx-target="#items" hx-swap="outerHTML" {
                                            select name="tag_id" aria-label="Tag" required {
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
                                            button { "Add" }
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
        }
    }
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

            form class="add" method="post" action={ "/lists/" (list.id.0) "/items" }
                 hx-post={ "/lists/" (list.id.0) "/items" }
                 hx-target="#items" hx-swap="outerHTML"
                 hx-on::after-request="this.reset()" {
                input type="text" name="name" placeholder="Add an item" required maxlength="128";
                input type="number" name="amount" value="1" min="0" step="any"
                      style="width:5rem" aria-label="Amount";
                select name="unit_id" aria-label="Unit" {
                    option value="" { "unit" }
                    @for (uid, uname) in &unit_names_sorted(&b.unit_names) {
                        option value=(uid) { (uname) }
                    }
                }
                button class="primary" { "Add" }
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
    Form(form): Form<NewItem>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;

    let unit_id = form
        .unit_id
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<i64>().ok())
        .map(unit::Id);

    items::create(
        &s.ctx,
        &actor,
        list::Id(id),
        Name(form.name),
        Amount(form.amount.unwrap_or(1.0)),
        unit_id,
    )
    .await?;

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

    swap(&s, &actor, &headers, list::Id(list_id), Some(item_id)).await
}

/// Re-renders the item board for htmx, or sends a browser back to the page.
/// Re-renders the item board for htmx, or sends a browser back to the page.
///
/// `open` keeps the panel the person is working in from closing under them: acting
/// inside it swaps the whole board, so the new markup has to say it was open.
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
