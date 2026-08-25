//! Notes: freeform reminders that are not on any particular list.

use axum::extract::{Form, State};
use axum::http::HeaderMap;
use axum::response::Response;
use domain::models::note::{self, Body};
use domain::models::{Direction, OrderBy, Paging};
use domain::service::{Actor, notes};
use maud::{Markup, html};
use tower_sessions::Session;

use crate::auth;
use crate::error::AppError;
use crate::htmx::swap_or_redirect;
use crate::state::AppState;
use crate::view;

#[derive(serde::Deserialize)]
pub struct NoteForm {
    pub body: String,
}

fn fragment(notes: &[note::Note]) -> Markup {
    html! {
        div id="notes" {
            @if notes.is_empty() {
                p class="empty" { "No notes yet." }
            } @else {
                ul class="rows" {
                    @for n in notes {
                        li {
                            span class="grow" { (n.body.0) }
                            form class="inline" method="post"
                                 action={ "/notes/" (n.id.0) "/delete" }
                                 hx-post={ "/notes/" (n.id.0) "/delete" }
                                 hx-target="#notes" hx-swap="outerHTML" {
                                button class="quiet" title="Delete" { "×" }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn current(s: &AppState, actor: &Actor) -> Result<Vec<note::Note>, AppError> {
    Ok(notes::list(
        &s.ctx,
        actor,
        Paging {
            number: 1,
            size: 100,
        },
        OrderBy {
            field: note::Field::CreatedAt,
            direction: Direction::Descending,
        },
    )
    .await?
    .items)
}

pub async fn index(session: Session, State(s): State<AppState>) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();
    let notes = current(&s, &actor).await?;

    Ok(view::page(
        "Notes",
        Some(&crate::pages::who(&user)),
        html! {
            (fragment(&notes))

            form class="add" method="post" action="/notes"
                 hx-post="/notes" hx-target="#notes" hx-swap="outerHTML"
                 hx-on::after-request="this.reset()" {
                input type="text" name="body" placeholder="Add a note" required maxlength="4096";
                button class="primary" { "Add" }
            }
        },
    ))
}

pub async fn create(
    session: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NoteForm>,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    notes::create(&s.ctx, &actor, Body(form.body)).await?;
    Ok(swap_or_redirect(
        &headers,
        fragment(&current(&s, &actor).await?),
        "/notes",
    ))
}

pub async fn delete(
    session: Session,
    State(s): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    notes::delete(&s.ctx, &actor, note::Id(id)).await?;
    Ok(swap_or_redirect(
        &headers,
        fragment(&current(&s, &actor).await?),
        "/notes",
    ))
}
