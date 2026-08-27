//! Who you are.
//!
//! Read-only. `users::update_profile` exists and is tested, but wiring it up would
//! ship a setting that silently does not stick: authentication resolves the identity
//! through `User::find_or_create` on *every* request, and that coalesces the
//! provider's claims over what is stored. Google sends a name and an address every
//! time, so a profile a person edited here would be overwritten by their next
//! request. Deciding which side wins is a real decision, and until it is made this
//! stays read-only rather than pretending.
//!
//! `users::close_account` is likewise not wired: it cascades away every list, item
//! and note the person owns, and an irreversible DELETE deserves a confirmation flow
//! designed on purpose rather than a route added in passing.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use domain::models::user::User;
use domain::service::users;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(me))
        .route("/events", get(events))
}

/// The signed-in person.
///
/// There is no `/api/users/{id}`: nothing here needs one person to look another up,
/// and an endpoint that could would be the first thing to leak an address.
/// The person, and the one thing about them that is not on the person.
///
/// `is_owner` is a fact about this *server*, not about them — the same account on
/// another server is not an owner of it — so it is beside the user rather than a field
/// on `User`. It is here at all so a client knows whether to offer the screen that
/// manages who may sign in.
#[derive(serde::Serialize)]
pub struct Me {
    #[serde(flatten)]
    user: User,
    is_owner: bool,
}

async fn me(State(state): State<AppState>, user: CurrentUser) -> Result<Json<Me>, AppError> {
    let actor = user.actor();
    let is_owner = domain::service::admission::is_owner(&state.ctx, actor.person()?.id).await?;

    // Through the service, not straight off the extractor: this is the only route
    // that could plausibly skip it, and the moment one does the rule stops being one.
    Ok(Json(Me {
        user: users::me(&state.ctx, &actor).await?,
        is_owner,
    }))
}

/// A stream that says when the set of lists this person can see has changed.
///
/// Separate from a list's own stream because it answers a different question. A list
/// that has just been made has no watchers, so announcing it to itself reaches
/// nobody — which is why a list made on one device never appeared on another.
///
/// Like every other event here it carries no content: the client re-reads through the
/// ordinary route, so there is one description of what a list is.
async fn events(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let me = user.actor().person()?.id;

    let watching = BroadcastStream::new(state.ctx.changes.watch_lists());
    let stream = watching.filter_map(move |heard| match heard {
        Ok(changed) if changed.user_id == me => {
            Some(Ok(Event::default().event("changed").data(me.0.to_string())))
        }
        // Lagging means events were dropped, which for a nudge is the same news as
        // one arriving.
        Err(_) => Some(Ok(Event::default().event("changed").data(me.0.to_string()))),
        Ok(_) => None,
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
