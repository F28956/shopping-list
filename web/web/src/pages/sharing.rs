//! Who a list is shared with.
//!
//! Reached from the lists index rather than from inside a list: sharing is something
//! you do *to* a list, alongside renaming and deleting it, not something you do while
//! shopping.

use axum::extract::{Path, State};
use axum::response::Redirect;
use domain::models::invite::Token;
use domain::models::list::{self, Role};
use domain::models::user;
use domain::service::lists;
use maud::{Markup, html};
use tower_sessions::Session;

use crate::error::AppError;
use crate::state::AppState;
use crate::{auth, view};

/// The share page for one list: who is on it, and how to add or remove somebody.
pub async fn show(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();
    let list = lists::get(&s.ctx, &actor, list::Id(id)).await?;
    let role = lists::role(&s.ctx, &actor, list::Id(id)).await?;
    let members = lists::members(&s.ctx, &actor, list::Id(id)).await?;

    Ok(view::page(
        &format!("Sharing {}", list.name.0),
        Some(&crate::pages::who(&user)),
        html! {
            p { a href="/" { "← all lists" } }
            h2 style="font-size:1.1rem;margin:.5rem 0 1rem" { "Sharing " (list.name.0) }

            @if members.is_empty() {
                p class="empty" { "This list is yours alone." }
            } @else {
                ul class="rows" {
                    @for m in &members {
                        li class="item" {
                            span class="grow" { "Someone" }
                            span class="amount" { (role_name(m.role)) }
                            @if role >= Role::Owner {
                                form class="inline" method="post"
                                     action={ "/lists/" (list.id.0) "/members/" (m.user_id.0) "/remove" } {
                                    button class="quiet" title="Remove them" { "×" }
                                }
                            }
                        }
                    }
                }
            }

            @if role >= Role::Owner {
                // One kind of link, so there is nothing to choose: whoever follows it
                // can add and tick off, which is what sharing a shopping list is for.
                // Read-only sharing exists in the service layer for when it is asked
                // for.
                form class="add" method="post" action={ "/lists/" (list.id.0) "/invites" } {
                    button class="primary" { "Create an invitation link" }
                }
                @if !members.is_empty() {
                    form class="inline" method="post"
                         action={ "/lists/" (list.id.0) "/invites/revoke" } {
                        button class="quiet" { "Cancel outstanding links" }
                    }
                }
            } @else {
                p class="truncated" { "Shared with you by someone else." }
                form class="inline" method="post" action={ "/lists/" (list.id.0) "/leave" } {
                    button class="danger" { "Leave this list" }
                }
            }
        },
    ))
}

/// Creates an invitation and shows the link once.
///
/// Once, because only its hash is stored — an owner who loses it makes another. The
/// page says so rather than letting somebody discover it.
pub async fn invite(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Markup, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    let user = actor.person()?.clone();
    let list = lists::get(&s.ctx, &actor, list::Id(id)).await?;

    let token = lists::invite(&s.ctx, &actor, list::Id(id), Role::Editor).await?;
    let link = format!("{}/join/{}", origin(), token.0);

    Ok(view::page(
        &format!("Sharing {}", list.name.0),
        Some(&crate::pages::who(&user)),
        html! {
            p { a href={ "/lists/" (list.id.0) "/share" } { "← sharing " (list.name.0) } }
            h2 style="font-size:1.1rem;margin:.5rem 0" { "Invitation link" }
            p { "Send this to whoever should join. It stops working after a week." }
            p class="token" { (link) }
            p class="truncated" {
                "This is the only time it is shown — nothing stores the link itself, "
                "only enough to recognise it. Lose it and make another."
            }
        },
    ))
}

/// Where the invitation link points.
fn origin() -> String {
    std::env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Editor => "can edit",
        Role::Viewer => "can look",
    }
}

pub async fn revoke(
    session: Session,
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::revoke_invites(&s.ctx, &actor, list::Id(id)).await?;
    Ok(Redirect::to(&format!("/lists/{id}/share")))
}

pub async fn remove_member(
    session: Session,
    State(s): State<AppState>,
    Path((id, who)): Path<(i64, i64)>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::remove_member(&s.ctx, &actor, list::Id(id), user::Id(who)).await?;
    Ok(Redirect::to(&format!("/lists/{id}/share")))
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
