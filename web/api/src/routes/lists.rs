use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use domain::models::OffsetPage;
use domain::models::list::{self, List, Name};
use domain::service::lists;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::routes::PageQuery;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(read).put(update).delete(delete))
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
) -> Result<Json<OffsetPage<List>>, AppError> {
    Ok(Json(
        lists::list(&state.ctx, &user.actor(), q.paging(), q.order_by()).await?,
    ))
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
) -> Result<Json<List>, AppError> {
    Ok(Json(
        lists::get(&state.ctx, &user.actor(), list::Id(id)).await?,
    ))
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
