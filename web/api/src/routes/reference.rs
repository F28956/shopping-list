//! Units and tags.
//!
//! **Units are read-only and tags are not.** Only `Actor::System` writes units, so a
//! write route for them could only ever answer with a refusal — better that it does
//! not exist. Tags are different: they are the vocabulary a server's lists are filed
//! under, and twenty-one aisles chosen once in a migration is not a decision to make
//! on behalf of every household that runs this. Whoever owns the server decides, and
//! `tags::writable` is where that is enforced.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
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
    Router::new()
        .route("/", get(list_tags).post(create_tag))
        .route("/{id}", axum::routing::patch(update_tag).delete(delete_tag))
}

/// A new aisle, or what an existing one should become.
///
/// `colour` and `emoji` are absent rather than empty when there is none — the model
/// turns an empty string into `None` on the way in, so both spellings mean the same.
#[derive(Debug, serde::Deserialize)]
pub struct TagInput {
    pub name: String,
    pub colour: Option<String>,
    pub emoji: Option<String>,
}

async fn create_tag(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(input): Json<TagInput>,
) -> Result<(StatusCode, Json<Tag>), AppError> {
    let made = tags::create(
        &state.ctx,
        &user.actor(),
        tag::Name(input.name),
        input.colour.map(tag::Colour),
        input.emoji.map(tag::Emoji),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(made)))
}

/// Renames an aisle, or changes its glyph.
///
/// A whole replacement rather than a patch of the fields given: leaving `emoji` out is
/// how somebody removes one, and a partial update would have no way to say that.
async fn update_tag(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Json(input): Json<TagInput>,
) -> Result<Json<Tag>, AppError> {
    Ok(Json(
        tags::update(
            &state.ctx,
            &user.actor(),
            tag::Id(id),
            tag::Name(input.name),
            input.colour.map(tag::Colour),
            input.emoji.map(tag::Emoji),
        )
        .await?,
    ))
}

/// Removes an aisle.
///
/// What is filed under it becomes unfiled, and it drops out of everyone's walking
/// order — both by `ON DELETE CASCADE`, which is the honest reading: the aisle is
/// gone, so nothing is in it and nobody walks it.
async fn delete_tag(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    tags::delete(&state.ctx, &user.actor(), tag::Id(id)).await?;
    Ok(StatusCode::NO_CONTENT)
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
