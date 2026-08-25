//! Units and tags: read-only over HTTP.
//!
//! There is deliberately no POST, PUT or DELETE. Only `Actor::System` may write
//! shared reference data and no request can produce one, so a write route could only
//! ever return a refusal — better that the route does not exist than that it exists
//! and always says no.

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use domain::models::OffsetPage;
use domain::models::tag::{self, Tag};
use domain::models::unit::{self, Unit};
use domain::service::{tags, units};

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::routes::PageQuery;
use crate::state::AppState;

pub fn units_router() -> Router<AppState> {
    Router::new().route("/", get(list_units))
}

pub fn tags_router() -> Router<AppState> {
    Router::new().route("/", get(list_tags))
}

async fn list_units(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<PageQuery<unit::Field>>,
) -> Result<Json<OffsetPage<Unit>>, AppError> {
    Ok(Json(
        units::list(&state.ctx, &user.actor(), q.paging(), q.order_by()).await?,
    ))
}

async fn list_tags(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<PageQuery<tag::Field>>,
) -> Result<Json<OffsetPage<Tag>>, AppError> {
    Ok(Json(
        tags::list(&state.ctx, &user.actor(), q.paging(), q.order_by()).await?,
    ))
}
