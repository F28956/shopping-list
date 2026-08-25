//! Your lists.

use axum::extract::{Form, Path, State};
use axum::response::Redirect;
use domain::models::list::{self, Name};
use domain::models::{Direction, OrderBy, Paging};
use domain::service::lists;
use maud::{Markup, html};
use tower_sessions::Session;

use crate::auth;
use crate::error::AppError;
use crate::state::AppState;
use crate::view;

#[derive(serde::Deserialize)]
pub struct NewList {
    pub name: String,
}

pub async fn index(session: Session, State(s): State<AppState>) -> Result<Markup, AppError> {
    let Some(user) = auth::current_user(&session, &s.ctx).await? else {
        return Ok(view::sign_in());
    };
    let actor = domain::service::Actor::User(user.clone());

    // Most recently touched first: the list you edited last is the one you are on.
    let page = lists::list(
        &s.ctx,
        &actor,
        Paging {
            number: 1,
            size: 100,
        },
        OrderBy {
            field: list::Field::UpdatedAt,
            direction: Direction::Descending,
        },
    )
    .await?;

    Ok(view::page(
        "Lists",
        Some(&crate::pages::who(&user)),
        html! {
            @if page.items.is_empty() {
                p class="empty" { "No lists yet. Start one below." }
            } @else {
                ul class="rows" {
                    @for l in &page.items {
                        li {
                            a class="grow" href={ "/lists/" (l.id.0) } { (l.name.0) }
                            form class="inline" method="post" action={ "/lists/" (l.id.0) "/delete" } {
                                button class="quiet" title="Delete list" { "×" }
                            }
                        }
                    }
                }
            }

            form class="add" method="post" action="/lists" {
                input type="text" name="name" placeholder="New list" required maxlength="128";
                button class="primary" { "Add list" }
            }
        },
    ))
}

pub async fn create(
    session: Session,
    State(s): State<AppState>,
    Form(form): Form<NewList>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::create(&s.ctx, &actor, Name(form.name)).await?;
    Ok(Redirect::to("/"))
}

pub async fn delete(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::delete(&s.ctx, &actor, list::Id(id)).await?;
    Ok(Redirect::to("/"))
}
