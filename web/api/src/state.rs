use sqlx::SqlitePool;
use std::sync::Arc;

use crate::jwks::Jwks;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub jwks: Arc<Jwks>,
    pub google_client_id: String,
}
