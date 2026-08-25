//! Your lists.

use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use domain::models::list::{self, List, Name};
use domain::models::user;
use domain::models::{Direction, OrderBy, Paging};
use domain::service::{Actor, lists};
use maud::{Markup, html};
use tower_sessions::Session;

use crate::error::AppError;
use crate::htmx::swap_or_redirect;
use crate::state::AppState;
use crate::{auth, view};

#[derive(serde::Deserialize)]
pub struct NewList {
    pub name: String,
}

fn everything() -> Paging {
    domain::service::everything()
}

/// Most recently touched first: the list you edited last is the one you are on.
fn newest_first() -> OrderBy<list::Field> {
    OrderBy {
        field: list::Field::UpdatedAt,
        direction: Direction::Descending,
    }
}

/// The part of the page that changes. Everything that edits a list re-renders exactly
/// this, so there is one description of what a list looks like rather than one per
/// operation.
///
/// The add form deliberately sits *outside* it: htmx replaces this element, and a
/// form that is replaced loses the cursor. Leaving it in place means you can add three
/// lists without touching the mouse.
fn fragment(lists: &[List], total: i64, truncated: bool, ctx: &Sharing) -> Markup {
    html! {
        div id="lists" {
            @if lists.is_empty() {
                p class="empty" { "No lists yet. Start one below." }
            } @else {
                @if truncated {
                    p class="truncated" { "Showing " (lists.len()) " of " (total) "." }
                }
                ul class="rows" {
                    @for l in lists {
                        li {
                            a class="grow" href={ "/lists/" (l.id.0) } { (l.name.0) }

                            // Sharing belongs here, beside renaming and deleting:
                            // things you do to a list rather than while shopping.
                            @let shared_with = ctx.counts.get(&l.id.0).copied().unwrap_or(0);
                            a class="quiet share-link" href={ "/lists/" (l.id.0) "/share" }
                              title=@if l.owner_id == ctx.me { "Share this list" }
                                    @else { "Shared with you" } {
                                @if l.owner_id != ctx.me { "shared with you" }
                                @else if shared_with > 0 { "shared · " (shared_with) }
                                @else { "share" }
                            }
                            details class="edit" {
                                summary title="Rename" { "✎" }
                                form class="add" method="post"
                                     action={ "/lists/" (l.id.0) "/rename" }
                                     hx-post={ "/lists/" (l.id.0) "/rename" }
                                     hx-target="#lists" hx-swap="outerHTML" {
                                        input type="text" name="name" value=(l.name.0)
                                              required maxlength="128" aria-label="List name";
                                        button { "Save" }
                                }
                            }
                            form class="inline" method="post"
                                 action={ "/lists/" (l.id.0) "/delete" }
                                 hx-post={ "/lists/" (l.id.0) "/delete" }
                                 hx-target="#lists" hx-swap="outerHTML"
                                 hx-confirm={ "Delete " (l.name.0) " and everything on it?" } {
                                button class="quiet" title="Delete list" { "×" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The lists, and whether there were more than one page of them.
async fn current(s: &AppState, actor: &Actor) -> Result<(Vec<List>, i64, bool), AppError> {
    let page = lists::for_user(&s.ctx, actor, everything(), newest_first()).await?;
    Ok((page.items, page.total, page.has_more))
}

/// What the index needs in order to say who a list belongs to.
struct Sharing {
    me: user::Id,
    /// How many people each list is shared with — one query, not one per row.
    counts: std::collections::HashMap<i64, i64>,
}

async fn sharing(s: &AppState, actor: &Actor) -> Result<Sharing, AppError> {
    Ok(Sharing {
        me: actor.person()?.id,
        counts: lists::share_counts(&s.ctx, actor).await?,
    })
}

pub async fn index(session: Session, State(s): State<AppState>) -> Result<Markup, AppError> {
    let Some(actor) = auth::current_actor(&session, &s.ctx).await? else {
        return Ok(view::sign_in());
    };
    let user = actor.person()?.clone();
    let (lists, total, truncated) = current(&s, &actor).await?;
    let sharing = sharing(&s, &actor).await?;

    Ok(view::page(
        "Lists",
        Some(&crate::pages::who(&user)),
        html! {
            (fragment(&lists, total, truncated, &sharing))

            form class="add" method="post" action="/lists"
                 hx-post="/lists" hx-target="#lists" hx-swap="outerHTML" {
                input type="text" name="name" placeholder="New list" required maxlength="128";
                button class="primary" { "Add list" }
            }
        },
    ))
}

pub async fn create(
    session: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NewList>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::create(&s.ctx, &actor, Name(form.name)).await?;
    Ok(swap_or_redirect(
        &headers,
        fragment_of(current(&s, &actor).await?, &sharing(&s, &actor).await?),
        "/",
    ))
}

/// Renaming, not transferring: `lists::update` writes the name and stamps
/// `updated_at`, and the owner is not writable at all.
pub async fn rename(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<NewList>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::update(&s.ctx, &actor, list::Id(id), Name(form.name)).await?;
    Ok(swap_or_redirect(
        &headers,
        fragment_of(current(&s, &actor).await?, &sharing(&s, &actor).await?),
        "/",
    ))
}

pub async fn delete(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::delete(&s.ctx, &actor, list::Id(id)).await?;
    Ok(swap_or_redirect(
        &headers,
        fragment_of(current(&s, &actor).await?, &sharing(&s, &actor).await?),
        "/",
    ))
}

/// Renders what [`current`] returned.
fn fragment_of((lists, total, truncated): (Vec<List>, i64, bool), sharing: &Sharing) -> Markup {
    fragment(&lists, total, truncated, sharing)
}
