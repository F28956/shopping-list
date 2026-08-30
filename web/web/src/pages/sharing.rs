//! Who a list is shared with.
//!
//! Reached from the lists index rather than from inside a list: sharing is something
//! you do *to* a list, alongside renaming and deleting it, not something you do while
//! shopping.

use crate::base;
use axum::Form;
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
            p { a href=(base::at("/lists")) { "← all lists" } }
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
    // In the fragment, after the `#`, and that is the whole point: a browser never
    // sends a fragment to a server. The token therefore appears in no access log, no
    // proxy log and no `Referer` header, on the way to a self-hosted server nobody
    // audits. The page at `/join` reads it back out of the address bar itself.
    let link = format!("{}/join#{}", origin(), token.0);

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
///
/// The origin and the base path, because a link that dropped the prefix would send
/// somebody to a host that serves something else entirely at `/join`.
fn origin() -> String {
    let origin = std::env::var("PUBLIC_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());
    format!("{}{}", origin.trim_end_matches('/'), base::get())
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
    Ok(Redirect::to(&base::at(&format!("/lists/{id}/share"))))
}

pub async fn remove_member(
    session: Session,
    State(s): State<AppState>,
    Path((id, who)): Path<(i64, i64)>,
) -> Result<Redirect, AppError> {
    let actor = auth::require_actor(&session, &s.ctx).await?;
    lists::remove_member(&s.ctx, &actor, list::Id(id), user::Id(who)).await?;
    Ok(Redirect::to(&base::at(&format!("/lists/{id}/share"))))
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

    // Forget it as well, or the next visit sends them back to a list they just left.
    forget_if_last(&session, id).await?;

    Ok(Redirect::to(&base::at("/lists")))
}

/// Drops the remembered list if it is this one.
pub(super) async fn forget_if_last(session: &Session, id: i64) -> Result<(), AppError> {
    if session.get::<i64>(crate::auth::LAST_LIST).await? == Some(id) {
        session.remove::<i64>(crate::auth::LAST_LIST).await?;
    }
    Ok(())
}

/// Where an invitation link lands.
///
/// Deliberately **not** behind sign-in. The token is in the fragment, which lives only
/// in the browser, so it has to reach a page before anything can be done with it --
/// and bouncing a signed-out visitor to Google first would throw the fragment away on
/// the way. So the page loads, the script hands the token back in a form post, and
/// [`join`] is the one that insists on knowing who this is.
pub async fn joining() -> Markup {
    view::page(
        "Joining a list",
        None,
        html! {
            h2 style="font-size:1.1rem;margin:.5rem 0" { "Joining a shared list" }
            // Submitted by app.js, which fills the token in from the fragment. The
            // field is visible only without scripting, where somebody has to paste
            // the whole link in themselves -- the browser will not read its own
            // address bar for them.
            form class="add" method="post" action=(base::at("/join")) {
                noscript {
                    p { "Paste the link you were sent." }
                    input type="text" name="token" placeholder="https://…/join#…"
                          autocomplete="off" required;
                }
                noscript { button class="primary" { "Join" } }
            }
            p id="joining" class="truncated" { "One moment…" }
        },
    )
}

/// What the fragment carried, handed back by the page at [`joining`].
#[derive(Debug, serde::Deserialize)]
pub struct Redemption {
    pub token: String,
}

/// Following an invitation link.
///
/// A signed-out visitor is not turned away: the token is put in their session and
/// picked up again the moment they come back from Google, because otherwise following
/// a share link on a device you have never signed in on simply loses the invitation.
pub async fn join(
    session: Session,
    State(s): State<AppState>,
    Form(redemption): Form<Redemption>,
) -> Result<Redirect, AppError> {
    // Without scripting the whole link is pasted, so take the fragment off it. Any
    // other shape is left alone and fails as an unknown token, which is what it is.
    let token = match redemption.token.rsplit_once('#') {
        Some((_, after)) => after.to_string(),
        None => redemption.token,
    };

    let Some(actor) = auth::current_actor(&session, &s.ctx).await? else {
        session.insert(auth::PENDING_INVITE, &token).await?;
        return Ok(Redirect::to(&base::at("/auth/login")));
    };

    let list = lists::join(&s.ctx, &actor, &Token(token)).await?;
    Ok(Redirect::to(&base::at(&format!("/lists/{}", list.id.0))))
}
