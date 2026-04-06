//! HTTP-обработчик POST /teach — уведомление после выполнения запроса на хосте (с таймингами).

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::query_canonical::canonical_query;
use crate::store::{Sample, TrainStore};

pub const TEACH_PATH: &str = "/teach";

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeachPayload {
    pub method: String,
    pub path: String,
    /// Сырая query с хоста (часто с `?`); в store пишется каноническая строка после разбора в Zona.
    pub query: Option<String>,
    /// Время начала запроса на хосте (UTC, мс).
    pub unix_ms: i64,
    /// Полное время обработки пайплайном (мс), измерено на стороне прокси.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub request_length: Option<u64>,
    #[serde(default)]
    pub response_length: Option<u64>,
    /// Мс от начала запроса до OnStarting ответа (серверный TTFB).
    #[serde(default)]
    pub response_ttfb_ms: Option<u64>,
    /// Энтропия Шеннона на байт (0…8) по префиксу тела запроса — маркер «формы» payload под TLS.
    #[serde(default)]
    pub request_entropy_bits: Option<f64>,
    #[serde(default)]
    pub request_ones_ratio: Option<f64>,
    #[serde(default)]
    pub request_byte_histogram16: Option<Vec<f64>>,
    #[serde(default)]
    pub response_entropy_bits: Option<f64>,
    #[serde(default)]
    pub response_ones_ratio: Option<f64>,
    #[serde(default)]
    pub response_byte_histogram16: Option<Vec<f64>>,
}

async fn teach(State(store): State<Arc<TrainStore>>, Json(body): Json<TeachPayload>) -> StatusCode {
    let duration_ms = body.duration_ms.unwrap_or(0);
    let status_code = body.status_code.unwrap_or(0);

    info!(
        target: "zona::teach",
        method = %body.method,
        path = %body.path,
        query_raw = ?body.query,
        query_canonical = ?canonical_query(body.query.as_deref()),
        unix_ms = body.unix_ms,
        duration_ms,
        status_code,
        request_entropy_bits = ?body.request_entropy_bits,
        response_entropy_bits = ?body.response_entropy_bits,
        response_ttfb_ms = ?body.response_ttfb_ms,
        "teach"
    );

    let key = TrainStore::route_key(&body.method, &body.path);
    let query = canonical_query(body.query.as_deref());
    store.record(
        key,
        Sample {
            duration_ms,
            status_code,
            unix_ms: body.unix_ms,
            query,
            request_length: body.request_length,
            response_length: body.response_length,
            response_ttfb_ms: body.response_ttfb_ms,
            request_entropy_bits: body.request_entropy_bits,
            request_ones_ratio: body.request_ones_ratio,
            request_byte_histogram16: body.request_byte_histogram16,
            response_entropy_bits: body.response_entropy_bits,
            response_ones_ratio: body.response_ones_ratio,
            response_byte_histogram16: body.response_byte_histogram16,
        },
    );

    StatusCode::NO_CONTENT
}

pub fn router(store: Arc<TrainStore>) -> Router {
    Router::new()
        .route(TEACH_PATH, post(teach))
        .with_state(store)
}
