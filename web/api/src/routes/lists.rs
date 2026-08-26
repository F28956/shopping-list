use std::convert::Infallible;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};
use domain::models::OffsetPage;
use domain::models::list::{self, List, Name, Role};
use domain::service::lists;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::routes::PageQuery;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(read).put(update).delete(delete))
        .route("/{id}/events", get(events))
}

/// A list, plus what the caller may do with it.
///
/// Without this a client cannot tell a list it owns from one shared with it as a
/// viewer, so it either offers every control and lets the server refuse them, or
/// hides controls the person is entitled to. The browser has always known — it reads
/// the role straight out of the service — and this is what lets an app know too.
///
/// Flattened, so it is the list's own shape with one field added.
#[derive(Debug, serde::Serialize)]
pub struct ListWithRole {
    #[serde(flatten)]
    pub list: List,
    pub role: Role,
}

impl ListWithRole {
    /// Owners are answered from the list itself; anyone else costs a lookup.
    ///
    /// `lists::role` would re-check readability for every row, having just read them
    /// through a call that established exactly that.
    async fn of(state: &AppState, actor: &domain::service::Actor, list: List) -> Self {
        let role = match actor.person() {
            Ok(user) if user.id == list.owner_id => Some(Role::Owner),
            Ok(_) => lists::role(&state.ctx, actor, list.id).await.ok(),
            Err(_) => None,
        };

        Self {
            // A member row that has gone missing under a list the caller could read
            // is not a reason to fail the request: viewer is the least it can be.
            role: role.unwrap_or(Role::Viewer),
            list,
        }
    }
}

/// A list's editable fields. A DTO rather than the model's newtype, so nothing
/// outside a route can conjure a `Name` that skipped normalisation.
#[derive(Debug, serde::Deserialize)]
pub struct ListName {
    pub name: String,
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<PageQuery<list::Field>>,
) -> Result<Json<OffsetPage<ListWithRole>>, AppError> {
    let actor = user.actor();
    let page = lists::for_user(&state.ctx, &actor, q.paging(), q.order_by()).await?;

    let mut items = Vec::with_capacity(page.items.len());
    for list in page.items {
        items.push(ListWithRole::of(&state, &actor, list).await);
    }

    Ok(Json(OffsetPage {
        items,
        total: page.total,
        total_pages: page.total_pages,
        has_more: page.has_more,
    }))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<ListName>,
) -> Result<(StatusCode, Json<List>), AppError> {
    let list = lists::create(&state.ctx, &user.actor(), Name(input.name)).await?;
    Ok((StatusCode::CREATED, Json(list)))
}

async fn read(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<ListWithRole>, AppError> {
    let actor = user.actor();
    let list = lists::get(&state.ctx, &actor, list::Id(id)).await?;
    Ok(Json(ListWithRole::of(&state, &actor, list).await))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<ListName>,
) -> Result<Json<List>, AppError> {
    Ok(Json(
        lists::update(&state.ctx, &user.actor(), list::Id(id), Name(input.name)).await?,
    ))
}

async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    lists::delete(&state.ctx, &user.actor(), list::Id(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A stream that says when this list changed, so a second device stops showing what
/// is no longer there.
///
/// The events carry a list id and nothing else. Sending the rows themselves would
/// make every watcher a second source of truth for order and content, and the first
/// dropped event would leave two devices confidently disagreeing. A watcher that is
/// only told "re-read" cannot drift.
///
/// Authorised once, on connect, by the same door as every other read. That is a
/// deliberate limit worth naming: revoking someone's access does not hang up a stream
/// they already hold, so they keep learning that *something* changed until they
/// reconnect. They learn nothing about what, and every actual read is checked again.
async fn events(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let list_id = list::Id(id);
    lists::get(&state.ctx, &user.actor(), list_id).await?;

    // Subscribed before the stream is handed back, so a change made between the
    // authorisation read above and the client's first re-read is not lost.
    let watching = BroadcastStream::new(state.ctx.changes.watch());

    let stream = watching.filter_map(move |heard| match heard {
        // The id that changed, not the id being watched. They are equal after the
        // filter, but writing the watched one makes the payload true by construction
        // rather than by the filter above still being there.
        Ok(changed) if changed.list_id == list_id => Some(Ok(Event::default()
            .event("changed")
            .data(changed.list_id.0.to_string()))),
        // Falling behind means events were dropped, which for a nudge is the same
        // news as one arriving: re-read. Ending the stream here would leave the
        // client silently stale, which is the failure this whole route exists to fix.
        Err(_) => Some(Ok(Event::default().event("changed").data(id.to_string()))),
        Ok(_) => None,
    });

    // Proxies and phone radios drop a connection that says nothing for long enough,
    // and a silent list is the normal case.
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
