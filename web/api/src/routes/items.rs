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
use domain::service::{ServiceError, items, tags};

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::routes::PageQuery;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        // Before the `{item_id}` route only for readability; matchit prefers the
        // static segment either way, so `done` can never be read as an item id.
        .route("/done", axum::routing::delete(clear_done))
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
    /// The row as the sender last saw it, when this edit was made against a copy
    /// rather than against the row on screen. Only a client replaying a queue sends
    /// it; see [`items::Seen`] for what it decides.
    pub seen: Option<SeenInput>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SeenInput {
    pub name: String,
    #[serde(default = "one")]
    pub amount: f64,
    pub unit_id: Option<i64>,
}

impl SeenInput {
    fn parts(self) -> items::Seen {
        items::Seen {
            name: Name(self.name),
            amount: Amount(self.amount),
            unit_id: self.unit_id.map(unit::Id),
        }
    }
}

/// What to add, either way a caller might mean it.
///
/// `line` is one typed string read the way a person means it -- "2 kg apples" -- and
/// `name` is the three fields spelled out. Two shapes rather than one, because they
/// are two different intentions: a client that means an item literally called
/// "1 kg bag of rice" has to be able to say so, and guessing from whether the other
/// fields happen to be present would take that away.
#[derive(Debug, serde::Deserialize)]
pub struct NewItem {
    pub line: Option<String>,
    pub name: Option<String>,
    /// What the device already calls this, when it named the row before the server
    /// heard of it. Absent on the online path, where the server names it.
    pub uuid: Option<item::Uuid>,
    #[serde(default = "one")]
    pub amount: f64,
    pub unit_id: Option<i64>,
}

fn one() -> f64 {
    1.0
}

impl ItemInput {
    fn parts(self) -> (Name, Amount, Option<unit::Id>, Option<items::Seen>) {
        (
            Name(self.name),
            Amount(self.amount),
            self.unit_id.map(unit::Id),
            self.seen.map(SeenInput::parts),
        )
    }
}

/// An item, plus what it is filed under.
///
/// The ids only: a client that groups by category already has the tags themselves,
/// and repeating a name, an emoji and a sort order on every row would be most of the
/// payload. Flattened, so this is the item's own shape with one field added and an
/// older client reading it sees no difference.
#[derive(Debug, serde::Serialize)]
pub struct TaggedItem {
    #[serde(flatten)]
    pub item: Item,
    pub tag_ids: Vec<i64>,
}

/// One page of a list's items, each with the tags it carries.
///
/// The tags come from one query for the whole page, not one per row -- the same call
/// the browser makes, for the same reason.
async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Query(q): Query<PageQuery<item::Field>>,
) -> Result<Json<OffsetPage<TaggedItem>>, AppError> {
    let list_id = list::Id(list_id);
    let actor = user.actor();

    let page = items::for_list(&state.ctx, &actor, list_id, q.paging(), q.order_by()).await?;
    let by_item = tags::for_list(&state.ctx, &actor, list_id).await?;

    Ok(Json(OffsetPage {
        items: page
            .items
            .into_iter()
            .map(|item| TaggedItem {
                // Ordered by `sort_order` already, which is what makes "the first
                // tag" mean the same thing here as on the page.
                tag_ids: by_item
                    .get(&item.id.0)
                    .map(|ts| ts.iter().map(|t| t.id.0).collect())
                    .unwrap_or_default(),
                item,
            })
            .collect(),
        total: page.total,
        total_pages: page.total_pages,
        has_more: page.has_more,
    }))
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Json(input): Json<NewItem>,
) -> Result<(StatusCode, Json<Item>), AppError> {
    let list_id = list::Id(list_id);
    let actor = user.actor();

    let item = match (input.line, input.name) {
        // Both is ambiguous, and picking one silently would mean a client with a
        // bug got an item it never asked for. (The reason does not reach the caller
        // -- the error body carries the status' canonical text and nothing else.)
        (Some(_), Some(_)) => return Err(AppError::Service(ServiceError::InvalidInput)),
        // Parsed in the service, never here: the browser posts a line through the
        // same function, and two parsers in two transports is how a phone and a
        // browser come to disagree about what `2 kg apples` means.
        (Some(line), None) => {
            items::quick_add(&state.ctx, &actor, list_id, input.uuid, &line).await?
        }
        (None, Some(name)) => {
            items::create(
                &state.ctx,
                &actor,
                list_id,
                None, Name(name),
                Amount(input.amount),
                input.unit_id.map(unit::Id),
            )
            .await?
        }
        // Neither: an item has to be called something.
        (None, None) => return Err(AppError::Service(ServiceError::InvalidInput)),
    };

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
    let (name, amount, unit_id, seen) = input.parts();
    Ok(Json(
        items::update(
            &state.ctx,
            &user.actor(),
            item::Id(item_id),
            name,
            amount,
            unit_id,
            seen,
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

/// How many rows a bulk operation took with it.
#[derive(Debug, serde::Serialize)]
pub struct Cleared {
    pub cleared: u64,
}

/// Which rows a sweep meant, for a client that is replaying one.
///
/// `?ids=3,7,11`. Absent means the live reading -- everything that is done right now --
/// which is what the button on screen wants. Present means "these, and only these",
/// which is what a queue that has been sitting in a pocket wants; see the service.
#[derive(Debug, Default, serde::Deserialize)]
pub struct ClearQuery {
    pub ids: Option<String>,
}

impl ClearQuery {
    /// Unparseable entries are dropped rather than refused. The caller is naming rows
    /// to delete, and the safe reading of "3,banana,11" is the two rows it definitely
    /// named -- never the whole list, which is what refusing and falling back to the
    /// live meaning would do.
    fn named(&self) -> Option<Vec<item::Id>> {
        self.ids.as_ref().map(|raw| {
            raw.split(',')
                .filter_map(|entry| entry.trim().parse().ok())
                .map(item::Id)
                .collect()
        })
    }
}

/// Clears everything already ticked off, in one request.
///
/// A route rather than the client deleting each row: emptying the trolley is one
/// intention, and N requests for it can half-succeed, leaving a list in a state the
/// person never asked for.
async fn clear_done(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(list_id): Path<i64>,
    Query(q): Query<ClearQuery>,
) -> Result<Json<Cleared>, AppError> {
    let named = q.named();
    let cleared = items::clear_done(
        &state.ctx,
        &user.actor(),
        list::Id(list_id),
        named.as_deref(),
    )
    .await?;
    Ok(Json(Cleared { cleared }))
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
