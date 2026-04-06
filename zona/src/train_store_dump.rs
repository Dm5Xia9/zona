//! GET `/train-store` — JSON-снимок накопленных teach-данных (как в `train_store.json`).

use std::sync::Arc;

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};

use crate::store::PersistedTrainStore;
use crate::TrainStore;

pub const TRAIN_STORE_DUMP_PATH: &str = "/train-store";

async fn train_store_dump(State(store): State<Arc<TrainStore>>) -> Json<PersistedTrainStore> {
    Json(store.persisted_snapshot())
}

pub fn router(store: Arc<TrainStore>) -> Router {
    Router::new()
        .route(TRAIN_STORE_DUMP_PATH, get(train_store_dump))
        .with_state(store)
}
