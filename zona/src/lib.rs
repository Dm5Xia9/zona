//! Zona — компаньон-сервер: teach после выполнения запроса и выдача профилей маскировки.

mod mask_profiles;
mod query_canonical;
mod signature_synth;
mod store;
mod teach;
mod train_store_dump;
mod training_status;

pub use mask_profiles::{
    MaskCharacteristics, MaskProfilesQuery, MaskProfilesResponse, MaskProfilesRouteEntry, MaskVariant,
    MASK_PROFILES_PATH,
};
pub use store::{PersistedTrainStore, Sample, TrainStore, TrainingDoneMode};
pub use teach::{TeachPayload, TEACH_PATH};
pub use signature_synth::{SignatureSamplesResponse, SIGNATURE_SAMPLES_PATH};
pub use train_store_dump::TRAIN_STORE_DUMP_PATH;
pub use training_status::{TrainingStatusResponse, TRAINING_STATUS_PATH};

use std::sync::Arc;

use axum::Router;
use tower_http::trace::TraceLayer;

/// Собранное приложение Axum (маршруты + общие слои).
pub fn app() -> Router {
    app_with_store(Arc::new(TrainStore::from_env()))
}

/// То же приложение с заданным хранилищем (удобно для тестов).
pub fn app_with_store(store: Arc<TrainStore>) -> Router {
    Router::new()
        .merge(teach::router(store.clone()))
        .merge(mask_profiles::router(store.clone()))
        .merge(signature_synth::router(store.clone()))
        .merge(train_store_dump::router(store.clone()))
        .merge(training_status::router(store))
        .layer(TraceLayer::new_for_http())
}
