//! `POST /api/sync` — everything a device did while it could not reach the server.
//!
//! One route rather than a replay through the ordinary ones, for three reasons the
//! REST routes cannot give: the device's own timestamp travels with each change, a
//! resend is recognised rather than merely harmless, and the answer tells the device
//! what its offline rows turned into. See [`domain::service::sync`].

use axum::{Json, Router, routing::post};
use domain::models::item::{self, Amount, Name};
use domain::models::{list, tag, unit};
use domain::service::items::Seen;
use domain::service::sync::{self, Applied, Operation, What};
use time::OffsetDateTime;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(sync))
}

/// A batch, in the order the device made it.
#[derive(Debug, serde::Deserialize)]
pub struct Batch {
    pub operations: Vec<OperationInput>,
}

/// One queued change on the wire.
///
/// `list` and `item` are UUIDs, never ids. A device that added something with no signal
/// has no id for it and never will until this route answers.
#[derive(Debug, serde::Deserialize)]
pub struct OperationInput {
    pub id: String,
    /// When the device says it happened, RFC 3339. Clamped by the service if it is
    /// implausibly far ahead; behind is always believed.
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub list: list::Uuid,
    #[serde(flatten)]
    pub what: WhatInput,
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WhatInput {
    /// Make the list itself. The only operation that does not name an item, and the
    /// only one that does not need its list to exist — which is what lets a device
    /// make a list with no signal at all.
    MakeList {
        name: String,
    },
    Add {
        item: item::Uuid,
        /// One typed string, read the way a person means it -- `2 kg apples`. The
        /// alternative is the three fields spelled out; see the create route for why
        /// both shapes exist rather than one being guessed from the other.
        line: Option<String>,
        name: Option<String>,
        #[serde(default = "one")]
        amount: f64,
        unit_id: Option<i64>,
    },
    SetDone {
        item: item::Uuid,
        done: bool,
    },
    Update {
        item: item::Uuid,
        name: String,
        #[serde(default = "one")]
        amount: f64,
        unit_id: Option<i64>,
        seen: Option<SeenInput>,
    },
    Delete {
        item: item::Uuid,
    },
    ClearDone {
        items: Vec<item::Uuid>,
    },
    /// File it under an aisle. `tag_id` and not a name: see `What::Tag`.
    AttachTag {
        item: item::Uuid,
        tag_id: i64,
    },
    /// Stop filing it there.
    DetachTag {
        item: item::Uuid,
        tag_id: i64,
    },
    /// The order this person walks this list in. Names no item.
    SetTagOrder {
        tag_ids: Vec<i64>,
    },
}

/// The row as the device saw it when an edit was made against it.
#[derive(Debug, serde::Deserialize)]
pub struct SeenInput {
    pub name: String,
    #[serde(default = "one")]
    pub amount: f64,
    pub unit_id: Option<i64>,
}

fn one() -> f64 {
    1.0
}

impl From<OperationInput> for Operation {
    fn from(input: OperationInput) -> Self {
        Operation {
            id: input.id,
            at: input.at,
            list: input.list,
            what: input.what.into(),
        }
    }
}

impl From<WhatInput> for What {
    fn from(input: WhatInput) -> Self {
        match input {
            WhatInput::Add {
                item,
                line,
                name,
                amount,
                unit_id,
            } => What::Add {
                item,
                line,
                name: name.map(Name),
                amount: Amount(amount),
                unit: unit_id.map(unit::Id),
            },
            WhatInput::MakeList { name } => What::MakeList {
                name: domain::models::list::Name(name),
            },
            WhatInput::SetDone { item, done } => What::SetDone { item, done },
            WhatInput::Update {
                item,
                name,
                amount,
                unit_id,
                seen,
            } => What::Update {
                item,
                name: Name(name),
                amount: Amount(amount),
                unit: unit_id.map(unit::Id),
                seen: seen.map(|s| Seen {
                    name: Name(s.name),
                    amount: Amount(s.amount),
                    unit_id: s.unit_id.map(unit::Id),
                }),
            },
            WhatInput::Delete { item } => What::Delete { item },
            WhatInput::ClearDone { items } => What::ClearDone { items },
            WhatInput::AttachTag { item, tag_id } => What::Tag {
                item,
                tag: tag::Id(tag_id),
                attached: true,
            },
            WhatInput::DetachTag { item, tag_id } => What::Tag {
                item,
                tag: tag::Id(tag_id),
                attached: false,
            },
            WhatInput::SetTagOrder { tag_ids } => What::SetTagOrder {
                tags: tag_ids.into_iter().map(tag::Id).collect(),
            },
        }
    }
}

/// What became of each operation, in the order they were sent.
#[derive(Debug, serde::Serialize)]
pub struct Replayed {
    pub operations: Vec<Applied>,
}

/// Replays a batch.
///
/// `200` even when every operation in it was refused. A refusal is an answer about one
/// change, not a failure of the request -- and a client that has to read a status code
/// to find out which of its twelve changes landed has been told nothing useful.
async fn sync(
    axum::extract::State(state): axum::extract::State<AppState>,
    user: CurrentUser,
    Json(batch): Json<Batch>,
) -> Result<Json<Replayed>, AppError> {
    let operations: Vec<Operation> = batch.operations.into_iter().map(Operation::from).collect();

    // Kept because `replay` consumes the batch and the answers do not carry the kind.
    // The kind is also the only part of an operation safe to put in a metric label:
    // everything else in one is somebody's shopping.
    let kinds: Vec<&'static str> = operations.iter().map(|op| op.what.kind()).collect();

    // A queue that will not drain is the hardest thing to diagnose from the outside,
    // and the counters say only how many were refused and why -- not which row. At
    // `debug` or `trace`, where the operator has already been told the log will hold
    // the contents of people's lists, the batch itself is here to be read.
    observability::contents!(operations = ?operations, "replaying a batch");

    let applied = sync::replay(&state.ctx, &user.actor(), operations).await?;
    observability::instruments::sync_replayed(&kinds, &applied);

    Ok(Json(Replayed {
        operations: applied,
    }))
}
