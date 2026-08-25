//! Items are nested under their list, because that is what authorises them: the URL
//! says which list, and the service checks the actor owns it.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use domain::models::OffsetPage;
use domain::models::item::{self, Amount, Item, Name};
use domain::models::tag;
use domain::models::{list, unit};
use domain::service::{items, tags};

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::routes::PageQuery;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{item_id}", get(read).put(update).delete(delete))
        .route("/{item_id}/done", post(tick).delete(untick))
        .route("/{item_id}/tags", get(item_tags).post(attach_tag))
        .route(
            "/{item_id}/tags/{tag_id}",
            axum::routing::delete(detach_tag),
        )
}

#[derive(Debug, serde::Deserialize)]
pub struct ItemInput {
    pub name: String,
    #[serde(default = "one")]
    pub amount: f64,
    pub unit_id: Option<i64>,
}

fn one() -> f64 {
    1.0
}

impl ItemInput {
    fn parts(self) -> (Name, Amount, Option<unit::Id>) {
        (
            Name(self.name),
            Amount(self.amount),
            self.unit_id.map(unit::Id),
        )
    }
}

async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Query(q): Query<PageQuery<item::Field>>,
) -> Result<Json<OffsetPage<Item>>, AppError> {
    Ok(Json(
        items::for_list(
            &state.ctx,
            &user.actor(),
            list::Id(list_id),
            q.paging(),
            q.order_by(),
        )
        .await?,
    ))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Json(input): Json<ItemInput>,
) -> Result<(StatusCode, Json<Item>), AppError> {
    let (name, amount, unit_id) = input.parts();
    let item = items::create(
        &state.ctx,
        &user.actor(),
        list::Id(list_id),
        name,
        amount,
        unit_id,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn read(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
) -> Result<Json<Item>, AppError> {
    Ok(Json(
        items::get(&state.ctx, &user.actor(), item::Id(item_id)).await?,
    ))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
    Json(input): Json<ItemInput>,
) -> Result<Json<Item>, AppError> {
    let (name, amount, unit_id) = input.parts();
    Ok(Json(
        items::update(
            &state.ctx,
            &user.actor(),
            item::Id(item_id),
            name,
            amount,
            unit_id,
        )
        .await?,
    ))
}

/// Ticking off is its own route rather than a field on the update body: it is the
/// one thing a client does constantly, and it should not have to send back the name
/// and amount to do it.
async fn tick(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
) -> Result<Json<Item>, AppError> {
    Ok(Json(
        items::set_done(&state.ctx, &user.actor(), item::Id(item_id), true).await?,
    ))
}

async fn untick(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
) -> Result<Json<Item>, AppError> {
    Ok(Json(
        items::set_done(&state.ctx, &user.actor(), item::Id(item_id), false).await?,
    ))
}

async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    items::delete(&state.ctx, &user.actor(), item::Id(item_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What is on this item.
#[derive(Debug, serde::Deserialize)]
pub struct TagRef {
    pub tag_id: i64,
}

async fn item_tags(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<tag::Tag>>, AppError> {
    Ok(Json(
        tags::for_item(&state.ctx, &user.actor(), item::Id(item_id)).await?,
    ))
}

/// Attaching is an edit to the item, so it is authorised by the item's list -- which
/// is why this lives under the item rather than under the tag.
async fn attach_tag(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id)): Path<(i64, i64)>,
    Json(input): Json<TagRef>,
) -> Result<StatusCode, AppError> {
    tags::attach(
        &state.ctx,
        &user.actor(),
        item::Id(item_id),
        tag::Id(input.tag_id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn detach_tag(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((_list_id, item_id, tag_id)): Path<(i64, i64, i64)>,
) -> Result<StatusCode, AppError> {
    tags::detach(
        &state.ctx,
        &user.actor(),
        item::Id(item_id),
        tag::Id(tag_id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
