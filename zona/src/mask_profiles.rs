//! GET /mask-profiles — пять срезов **наблюдаемого** распределения X(params) (эталон с хоста teach).
//!
//! Без `method`/`path` — только маршруты с достаточным числом сэмплов (каждый — пять вариантов; неготовые ключи не включаются). С `method` и `path` — один маршрут или пустой `routes`.
//!
//! Это не инструкции к текущему коду приложения: маскирующая функция Y(params) пока не подключена.
//! Поля — статистические ориентиры, с которыми позже нужно согласовывать Y (тайминги, форма payload, размеры).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::store::{Sample, TrainStore};

pub const MASK_PROFILES_PATH: &str = "/mask-profiles";

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskProfilesQuery {
    /// Вместе с `path` — один ключ `METHOD|path`; если оба пусты — отдельные профили **по каждому** ключу в store.
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// Устар.: ключ не зависит от query; параметр игнорируется.
    #[serde(default)]
    pub query: Option<String>,
}

fn is_specific_route_key(q: &MaskProfilesQuery) -> bool {
    let m = q
        .method
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let p = q.path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    m.is_some() && p.is_some()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskProfilesResponse {
    pub min_samples: usize,
    pub routes: Vec<MaskProfilesRouteEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskProfilesRouteEntry {
    pub route_key: String,
    pub sample_count: usize,
    pub ready: bool,
    pub variants: Vec<MaskVariant>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskVariant {
    pub id: u8,
    pub label: String,
    pub characteristics: MaskCharacteristics,
    /// 0–100: насколько профиль близок к «ядру» наблюдаемого распределения (выше — естественнее для DPI).
    pub masking_percent: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskCharacteristics {
    /// Квантиль `p` длительностей обработки X на хосте (мс) — временной профиль эталона.
    pub target_latency_ms: f64,
    /// Характерный разброс задержек по выборке (мс), IQR×0.25 — не команда «добавить джиттер», а описание X.
    pub latency_jitter_ms: f64,
    /// Квантиль `p` по длине канонической query-строки (байты, без `?`) из teach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_query_length_bytes: Option<u64>,
    /// Квантиль `p` по длинам **тела** запроса (Content-Length teach), не включая query в URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_request_length_bytes: Option<u64>,
    /// Квантиль `p` по длинам тела ответа X.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_response_length_bytes: Option<u64>,
    /// Квантиль `p` по наблюдаемому TTFB (мс до OnStarting ответа на хосте teach).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_response_ttfb_ms: Option<f64>,
    /// Квантили метрик «формы» байтов teach по выборке X (для будущего сопоставления с Y).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_request_entropy_bits: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_response_entropy_bits: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_request_ones_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_response_ones_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_request_byte_histogram16: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_response_byte_histogram16: Option<Vec<f64>>,
}

/// Пять квантилей задержек для вариантов профиля.
const LATENCY_PERCENTILES: [f64; 5] = [12.0, 28.0, 50.0, 72.0, 88.0];

const LABELS: [&str; 5] = [
    "Нижний хвост (быстрее)",
    "Ниже медианы",
    "Медиана (ядро)",
    "Выше медианы",
    "Верхний хвост (медленнее)",
];

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let p = p.clamp(0.0, 100.0);
    let idx = (sorted.len() - 1) as f64 * p / 100.0;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let w = idx - lo as f64;
    sorted[lo] * (1.0 - w) + sorted[hi] * w
}

/// Оценка «маскировки»: 100 у медианы цели, падает по мере удаления от медианы выборки.
fn masking_percent_for_latency(target: f64, sorted_durations: &[f64]) -> f64 {
    if sorted_durations.is_empty() {
        return 0.0;
    }
    let med = percentile(sorted_durations, 50.0);
    let p10 = percentile(sorted_durations, 10.0);
    let p90 = percentile(sorted_durations, 90.0);
    let half_width = ((p90 - p10) * 0.5).max(1.0);
    let z = (target - med).abs() / half_width;
    // Гауссоподобное ядро: в центре ~100, на ~2 half-width уже мало
    let score = 100.0 * (-0.5 * z * z).exp();
    (score * 10.0).round() / 10.0
}

fn collect_percentile(samples: &[Sample], p: f64, extract: impl Fn(&Sample) -> Option<f64>) -> Option<f64> {
    let mut v: Vec<f64> = samples.iter().filter_map(extract).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(percentile(&v, p))
}

fn collect_query_length_percentile(samples: &[Sample], p: f64) -> Option<u64> {
    let v: Vec<u64> = samples
        .iter()
        .map(|s| s.query.as_ref().map(|q| q.len() as u64).unwrap_or(0))
        .collect();
    if v.is_empty() {
        return None;
    }
    let mut vf: Vec<f64> = v.iter().map(|&x| x as f64).collect();
    vf.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let x = percentile(&vf, p);
    Some(x.round().max(0.0) as u64)
}

fn collect_length_percentile(samples: &[Sample], p: f64, request: bool) -> Option<u64> {
    let v: Vec<u64> = samples
        .iter()
        .filter_map(|s| {
            if request {
                s.request_length
            } else {
                s.response_length
            }
        })
        .collect();
    if v.is_empty() {
        return None;
    }
    let mut vf: Vec<f64> = v.iter().map(|&x| x as f64).collect();
    vf.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let x = percentile(&vf, p);
    Some(x.round().max(0.0) as u64)
}

fn collect_histogram_percentile(samples: &[Sample], p: f64, request: bool) -> Option<Vec<f64>> {
    let rows: Vec<&Vec<f64>> = samples
        .iter()
        .filter_map(|s| {
            let h = if request {
                s.request_byte_histogram16.as_ref()
            } else {
                s.response_byte_histogram16.as_ref()
            };
            h.filter(|x| x.len() == 16)
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(16);
    for bin in 0..16 {
        let mut col: Vec<f64> = rows.iter().map(|h| h[bin]).collect();
        col.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out.push(percentile(&col, p));
    }
    Some(out)
}

pub(crate) fn build_variants(samples: &[Sample]) -> Vec<MaskVariant> {
    let mut durations: Vec<f64> = samples.iter().map(|s| s.duration_ms as f64).collect();
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let jitter_base = {
        let q1 = percentile(&durations, 25.0);
        let q3 = percentile(&durations, 75.0);
        ((q3 - q1) * 0.25).max(1.0)
    };

    LATENCY_PERCENTILES
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let target = percentile(&durations, p);
            let masking = masking_percent_for_latency(target, &durations);
            MaskVariant {
                id: (i + 1) as u8,
                label: LABELS[i].to_string(),
                characteristics: MaskCharacteristics {
                    target_latency_ms: (target * 10.0).round() / 10.0,
                    latency_jitter_ms: (jitter_base * 10.0).round() / 10.0,
                    observed_query_length_bytes: collect_query_length_percentile(samples, p),
                    observed_request_length_bytes: collect_length_percentile(samples, p, true),
                    observed_response_length_bytes: collect_length_percentile(samples, p, false),
                    observed_response_ttfb_ms: collect_percentile(samples, p, |s| {
                        s.response_ttfb_ms.map(|x| x as f64)
                    })
                    .map(|x| (x * 10.0).round() / 10.0),
                    target_request_entropy_bits: collect_percentile(samples, p, |s| s.request_entropy_bits),
                    target_response_entropy_bits: collect_percentile(samples, p, |s| s.response_entropy_bits),
                    target_request_ones_ratio: collect_percentile(samples, p, |s| s.request_ones_ratio),
                    target_response_ones_ratio: collect_percentile(samples, p, |s| s.response_ones_ratio),
                    target_request_byte_histogram16: collect_histogram_percentile(samples, p, true),
                    target_response_byte_histogram16: collect_histogram_percentile(samples, p, false),
                },
                masking_percent: masking,
            }
        })
        .collect()
}

fn entry_for_samples(route_key: String, samples: Vec<Sample>, min: usize) -> MaskProfilesRouteEntry {
    let count = samples.len();
    let ready = count >= min;
    let variants = if ready {
        build_variants(&samples)
    } else {
        vec![]
    };
    MaskProfilesRouteEntry {
        route_key,
        sample_count: count,
        ready,
        variants,
    }
}

async fn mask_profiles(
    State(store): State<std::sync::Arc<TrainStore>>,
    Query(q): Query<MaskProfilesQuery>,
) -> Result<Json<MaskProfilesResponse>, StatusCode> {
    let min = store.min_samples;

    let routes = if is_specific_route_key(&q) {
        let m = q.method.as_deref().map(str::trim).unwrap_or("");
        let p = q.path.as_deref().map(str::trim).unwrap_or("");
        let key = TrainStore::route_key(m, p);
        let samples = store.samples_for(&key).unwrap_or_default();
        let e = entry_for_samples(key, samples, min);
        if e.ready {
            vec![e]
        } else {
            vec![]
        }
    } else {
        store
            .routes_with_samples()
            .into_iter()
            .filter_map(|(key, samples)| {
                let e = entry_for_samples(key, samples, min);
                e.ready.then_some(e)
            })
            .collect()
    };

    Ok(Json(MaskProfilesResponse { min_samples: min, routes }))
}

pub fn router(store: std::sync::Arc<TrainStore>) -> Router {
    Router::new()
        .route(MASK_PROFILES_PATH, get(mask_profiles))
        .with_state(store)
}
