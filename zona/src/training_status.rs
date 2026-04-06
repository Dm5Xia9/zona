//! GET /training-status — режим обучения для хоста (когда перестать слать teach).

use std::sync::Arc;

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::store::{TrainStore, TrainingDoneMode};

pub const TRAINING_STATUS_PATH: &str = "/training-status";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingStatusResponse {
    pub training_complete: bool,
    pub done_mode: &'static str,
    pub total_samples: usize,
    pub route_count: usize,
    /// Сэмплов на ключ для «готовности» ключа в `/mask-profiles` и для режима `all_routes`.
    pub min_samples_per_route: usize,
    /// В режиме `all_routes`: сколько ключей должны набрать ≥ `min_samples_per_route`.
    pub min_ready_routes: usize,
    /// Сколько ключей уже набрали ≥ `min_samples_per_route`.
    pub routes_meeting_min_samples: usize,
    /// Имеет смысл в режиме `total`: глобальный порог суммы сэмплов.
    pub total_threshold: usize,
    /// Минимум сэмплов среди всех ключей (узкое место по одному маршруту).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_route_sample_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_unix_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_directory: Option<String>,
}

fn mode_str(m: TrainingDoneMode) -> &'static str {
    match m {
        TrainingDoneMode::Total => "total",
        TrainingDoneMode::AllRoutes => "allRoutes",
    }
}

async fn training_status(State(store): State<Arc<TrainStore>>) -> Json<TrainingStatusResponse> {
    Json(TrainingStatusResponse {
        training_complete: store.training_complete(),
        done_mode: mode_str(store.done_mode()),
        total_samples: store.total_sample_count(),
        route_count: store.route_count(),
        min_samples_per_route: store.min_samples,
        min_ready_routes: store.min_ready_routes,
        routes_meeting_min_samples: store.routes_meeting_min_samples(),
        total_threshold: store.total_threshold(),
        min_route_sample_count: store.min_route_sample_count(),
        completed_at_unix_ms: store.completed_at_unix_ms(),
        data_directory: store.data_directory_display(),
    })
}

pub fn router(store: Arc<TrainStore>) -> Router {
    Router::new()
        .route(TRAINING_STATUS_PATH, get(training_status))
        .with_state(store)
}
