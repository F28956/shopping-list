//! Notes: freeform reminders that are not on any particular list.

use axum::extract::{Form, State};
use axum::response::Redirect;
use domain::models::note::{self, Body};
use domain::models::{Direction, OrderBy, Paging};
use domain::service::notes;
use maud::{Markup, html};
use tower_sessions::Session;

use crate::auth;
use crate::error::AppError;
use crate::state::AppState;
use crate::view;

#[derive(serde::Deserialize)]
pub struct NoteForm {
    pub body: String,
}

pub async fn index(session: Session, State(s): State<AppState>) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();

    let page = notes::list(
        &s.ctx,
        &actor,
        Paging {
            number: 1,
            size: 100,
        },
        OrderBy {
            field: note::Field::CreatedAt,
            direction: Direction::Descending,
        },
    )
    .await?;

    Ok(view::page(
        "Notes",
        Some(&crate::pages::who(&user)),
        html! {
            @if page.items.is_empty() {
                p class="empty" { "No notes yet." }
            } @else {
                ul class="rows" {
                    @for n in &page.items {
                        li {
                            span class="grow" { (n.body.0) }
                            form class="inline" method="post" action={ "/notes/" (n.id.0) "/delete" } {
                                button class="quiet" title="Delete" { "×" }
                            }
                        }
                    }
                }
            }

            form class="add" method="post" action="/notes" {
                input type="text" name="body" placeholder="Add a note" required maxlength="4096";
                button class="primary" { "Add" }
            }
        },
    ))
}

pub async fn create(
    session: Session,
    State(s): State<AppState>,
    Form(form): Form<NoteForm>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    notes::create(&s.ctx, &actor, Body(form.body)).await?;
    Ok(Redirect::to("/notes"))
}

pub async fn delete(
    session: Session,
    State(s): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    notes::delete(&s.ctx, &actor, note::Id(id)).await?;
    Ok(Redirect::to("/notes"))
}
