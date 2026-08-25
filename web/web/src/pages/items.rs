//! What is on one list.

use axum::extract::{Form, Path, State};
use axum::response::Redirect;
use domain::models::item::{self, Amount, Name};
use domain::models::{Direction, OrderBy, Paging, list, tag, unit};
use domain::service::{items, lists, tags, units};
use maud::{Markup, html};
use tower_sessions::Session;

use crate::auth;
use crate::error::AppError;
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

    let page = items::for_list(
        &s.ctx,
        &actor,
        list.id,
        everything(),
        // Outstanding first, then the ones already ticked off.
        OrderBy {
            field: item::Field::DoneAt,
            direction: Direction::Ascending,
        },
    )
    .await?;

    let unit_names = unit_lookup(&s, &actor).await?;
    // One query for the whole page rather than one per item.
    let tags_by_item = tags::on_list(&s.ctx, &actor, list.id).await?;
    let all_tags = tags::list(
        &s.ctx,
        &actor,
        everything(),
        OrderBy {
            field: tag::Field::Name,
            direction: Direction::Ascending,
        },
    )
    .await?
    .items;

    Ok(view::page(
        &list.name.0,
        Some(&crate::pages::who(&user)),
        html! {
            p { a href="/" { "← all lists" } }
            h2 style="font-size:1.1rem;margin:.5rem 0 1rem" { (list.name.0) }

            @if page.items.is_empty() {
                p class="empty" { "Nothing on this list yet." }
            } @else {
                ul class="rows" {
                    @for i in &page.items {
                        li class=@if i.done_at.is_some() { "done" } @else { "" } {
                            form class="inline" method="post"
                                 action={ "/lists/" (list.id.0) "/items/" (i.id.0) "/toggle" } {
                                button class="quiet" title="Tick off" {
                                    @if i.done_at.is_some() { "☑" } @else { "☐" }
                                }
                            }
                            span class="grow" { (i.name.0) }
                            span class="amount" {
                                (trim_amount(i.amount))
                                @if let Some(u) = i.unit_id.and_then(|u| unit_names.get(&u.0)) {
                                    " " (u)
                                }
                            }
                            form class="inline" method="post"
                                 action={ "/lists/" (list.id.0) "/items/" (i.id.0) "/delete" } {
                                button class="quiet" title="Remove" { "×" }
                            }
                        }
                        li class="tagrow" {
                            @let on_item = tags_by_item.get(&i.id.0);
                            @for t in on_item.into_iter().flatten() {
                                form class="inline" method="post"
                                     action={ "/lists/" (list.id.0) "/items/" (i.id.0)
                                              "/tags/" (t.id.0) "/delete" } {
                                    button class="chip" title="Remove tag" {
                                        @if let Some(e) = &t.emoji { (e.0) " " }
                                        (t.name.0) " ×"
                                    }
                                }
                            }
                            details class="edit" {
                                summary title="Edit" { "✎ edit" }
                                form class="add" method="post"
                                     action={ "/lists/" (list.id.0) "/items/" (i.id.0) "/edit" } {
                                    input type="text" name="name" value=(i.name.0)
                                          required maxlength="128" aria-label="Item name";
                                    input type="number" name="amount" value=(trim_amount(i.amount))
                                          min="0" step="any" style="width:5rem" aria-label="Amount";
                                    select name="unit_id" aria-label="Unit" {
                                        option value="" selected[i.unit_id.is_none()] { "unit" }
                                        @for (uid, uname) in &unit_names_sorted(&unit_names) {
                                            option value=(uid)
                                                   selected[i.unit_id.map(|u| u.0) == Some(*uid)] {
                                                (uname)
                                            }
                                        }
                                    }
                                    button { "Save" }
                                }
                            }
                            details {
                                summary { "+ tag" }
                                form class="add" method="post"
                                     action={ "/lists/" (list.id.0) "/items/" (i.id.0) "/tags" } {
                                    select name="tag_id" aria-label="Tag" required {
                                        @for t in &all_tags {
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
                        }
                    }
                }
            }

            form class="add" method="post" action={ "/lists/" (list.id.0) "/items" } {
                input type="text" name="name" placeholder="Add an item" required maxlength="128";
                input type="number" name="amount" value="1" min="0" step="any"
                      style="width:5rem" aria-label="Amount";
                select name="unit_id" aria-label="Unit" {
                    option value="" { "unit" }
                    @for (id, name) in &unit_names_sorted(&unit_names) {
                        option value=(id) { (name) }
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
    Form(form): Form<NewItem>,
) -> Result<Redirect, AppError> {
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

    Ok(Redirect::to(&format!("/lists/{id}")))
}

/// One button that flips whichever way the item currently is — a browser form cannot
/// send a PUT, and asking the page to remember which state it was in invites the two
/// to disagree.
pub async fn toggle(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;

    let item = items::get(&s.ctx, &actor, item::Id(item_id)).await?;
    items::set_done(&s.ctx, &actor, item.id, item.done_at.is_none()).await?;

    Ok(Redirect::to(&format!("/lists/{list_id}")))
}

pub async fn delete(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    items::delete(&s.ctx, &actor, item::Id(item_id)).await?;
    Ok(Redirect::to(&format!("/lists/{list_id}")))
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
    Form(form): Form<TagChoice>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    tags::attach(&s.ctx, &actor, item::Id(item_id), tag::Id(form.tag_id)).await?;
    Ok(Redirect::to(&format!("/lists/{list_id}")))
}

pub async fn detach_tag(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id, tag_id)): Path<(i64, i64, i64)>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    tags::detach(&s.ctx, &actor, item::Id(item_id), tag::Id(tag_id)).await?;
    Ok(Redirect::to(&format!("/lists/{list_id}")))
}

/// Edits what a person typed: name, amount, unit. Not the list it is on -- moving an
/// item between lists would need the destination checked too, and is its own
/// operation rather than a field on this form.
pub async fn edit(
    session: Session,
    State(s): State<AppState>,
    Path((list_id, item_id)): Path<(i64, i64)>,
    Form(form): Form<NewItem>,
) -> Result<Redirect, AppError> {
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

    Ok(Redirect::to(&format!("/lists/{list_id}")))
}
