//! GET `/signature-samples` — **примеры** конкретного запроса/ответа в виде обычных строк (REST: query и текст ответа).
//! Синтез байтов под гистограмму/биты — внутри; наружу: query как печатный `a=1&b=2`…, тело и ответ — UTF-8 (lossy при необходимости).

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use rand::Rng;
use serde::Serialize;

use crate::mask_profiles::{build_variants, MaskCharacteristics, MaskProfilesQuery, MaskVariant};
use crate::store::{Sample, TrainStore};

pub const SIGNATURE_SAMPLES_PATH: &str = "/signature-samples";

const RESPONSE_END_MARKER: &[u8] = b"Hello World!#";

/// Печатные символы, похожие на query-string (ASCII, длина в байтах = `n`).
const QUERY_ALPHABET: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789=&%._-";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSamplesResponse {
    pub min_samples: usize,
    pub routes: Vec<SignatureRouteEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureRouteEntry {
    pub route_key: String,
    pub sample_count: usize,
    pub ready: bool,
    pub variants: Vec<SignatureVariantDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureVariantDto {
    pub variant_id: u8,
    pub label: String,
    /// Пример фрагмента query (как после `?`), без ведущего `?`.
    pub request_query_signature: String,
    /// Пример тела запроса (как текст; бинарь — через lossy UTF-8).
    pub request_body_signature: String,
    /// Сводный пример: `?{query}` + тело (или только одно из них).
    pub request_signature: String,
    /// Пример тела ответа (префикс под статистику + `Hello World!#`).
    pub response_signature: String,
    pub request_query_length_bytes: u64,
    pub request_body_length_bytes: u64,
    pub request_length_bytes: u64,
    pub response_length_bytes: u64,
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

fn synth_stream(n: usize, c: &MaskCharacteristics, use_response_stats: bool) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }

    let (ones_ratio, hist_opt) = if use_response_stats {
        (c.target_response_ones_ratio, c.target_response_byte_histogram16.as_ref())
    } else {
        (c.target_request_ones_ratio, c.target_request_byte_histogram16.as_ref())
    };

    let mut rng = rand::thread_rng();
    let mut out = Vec::with_capacity(n);

    let hist: [f64; 16] = if let Some(h) = hist_opt {
        if h.len() == 16 {
            let s: f64 = h.iter().sum();
            if s > 1e-12 {
                let mut a = [0f64; 16];
                for i in 0..16 {
                    a[i] = h[i] / s;
                }
                a
            } else {
                [1.0 / 16.0; 16]
            }
        } else {
            [1.0 / 16.0; 16]
        }
    } else {
        [1.0 / 16.0; 16]
    };

    for _ in 0..n {
        let r: f64 = rng.gen_range(0.0..1.0);
        let mut acc = 0.0;
        let mut bin = 15usize;
        for i in 0..16 {
            acc += hist[i];
            if r < acc || i == 15 {
                bin = i;
                break;
            }
        }
        let lo = (bin * 16) as u8;
        out.push(rng.gen_range(lo..=lo.saturating_add(15)));
    }

    if let Some(ratio) = ones_ratio {
        if ratio.is_finite() && (0.0..=1.0).contains(&ratio) {
            adjust_ones_ratio(&mut out, ratio);
        }
    }

    out
}

fn count_one_bits(data: &[u8]) -> usize {
    data.iter().map(|b| b.count_ones() as usize).sum()
}

fn adjust_ones_ratio(buf: &mut [u8], target_ratio: f64) {
    let total_bits = buf.len() * 8;
    if total_bits == 0 {
        return;
    }
    let want = (target_ratio * total_bits as f64).round() as isize;
    let have = count_one_bits(buf) as isize;
    let mut diff = want - have;
    let mut rng = rand::thread_rng();
    let mut guard = 0usize;
    while diff != 0 && guard < total_bits * 4 {
        guard += 1;
        let i = rng.gen_range(0..buf.len());
        let bit = rng.gen_range(0..8);
        let mask = 1u8 << bit;
        let is_one = (buf[i] & mask) != 0;
        if diff > 0 && !is_one {
            buf[i] |= mask;
            diff -= 1;
        } else if diff < 0 && is_one {
            buf[i] &= !mask;
            diff += 1;
        }
    }
}

/// Query как читаемая ASCII-строка той же байтовой длины (статистика из `synth_stream` → символы алфавита).
fn synth_query_text(n: usize, c: &MaskCharacteristics) -> String {
    if n == 0 {
        return String::new();
    }
    let raw = synth_stream(n, c, false);
    raw.iter()
        .map(|&b| QUERY_ALPHABET[b as usize % QUERY_ALPHABET.len()] as char)
        .collect()
}

fn synth_body_text(n: usize, c: &MaskCharacteristics) -> String {
    String::from_utf8_lossy(&synth_stream(n, c, false)).into_owned()
}

fn synth_response(n: u64, c: &MaskCharacteristics) -> Vec<u8> {
    let n = n as usize;
    if n == 0 {
        return Vec::new();
    }
    let tail_len = RESPONSE_END_MARKER.len();
    if n < tail_len {
        return synth_stream(n, c, true);
    }
    let prefix_len = n - tail_len;
    let mut out = synth_stream(prefix_len, c, true);
    out.extend_from_slice(RESPONSE_END_MARKER);
    out
}

fn synth_response_text(n: u64, c: &MaskCharacteristics) -> String {
    String::from_utf8_lossy(&synth_response(n, c)).into_owned()
}

fn build_request_example(query: &str, body: &str) -> String {
    match (query.is_empty(), body.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("?{}", query),
        (true, false) => body.to_string(),
        (false, false) => format!("?{}\n{}", query, body),
    }
}

fn entry_for_route(
    route_key: String,
    samples: Vec<Sample>,
    min: usize,
) -> Option<SignatureRouteEntry> {
    let sample_count = samples.len();
    if sample_count < min {
        return None;
    }
    let variants: Vec<SignatureVariantDto> = build_variants(&samples)
        .iter()
        .filter_map(variant_to_dto)
        .collect();
    Some(SignatureRouteEntry {
        route_key,
        sample_count,
        ready: true,
        variants,
    })
}

fn variant_to_dto(v: &MaskVariant) -> Option<SignatureVariantDto> {
    let c = &v.characteristics;
    let query_len = c.observed_query_length_bytes.unwrap_or(0);
    let body_len = c.observed_request_length_bytes.unwrap_or(0);
    let request_query_signature = synth_query_text(query_len as usize, c);
    let request_body_signature = synth_body_text(body_len as usize, c);
    let request_signature = build_request_example(&request_query_signature, &request_body_signature);
    let res_len = c.observed_response_length_bytes.unwrap_or(0);
    let response_signature = synth_response_text(res_len, c);
    Some(SignatureVariantDto {
        variant_id: v.id,
        label: v.label.clone(),
        request_query_signature,
        request_body_signature,
        request_signature,
        response_signature,
        request_query_length_bytes: query_len,
        request_body_length_bytes: body_len,
        request_length_bytes: query_len.saturating_add(body_len),
        response_length_bytes: res_len,
    })
}

async fn signature_samples(
    State(store): State<Arc<TrainStore>>,
    Query(q): Query<MaskProfilesQuery>,
) -> Result<Json<SignatureSamplesResponse>, StatusCode> {
    let min = store.min_samples;

    let routes = if is_specific_route_key(&q) {
        let m = q.method.as_deref().map(str::trim).unwrap_or("");
        let p = q.path.as_deref().map(str::trim).unwrap_or("");
        let key = TrainStore::route_key(m, p);
        let samples = store.samples_for(&key).unwrap_or_default();
        entry_for_route(key, samples, min).into_iter().collect()
    } else {
        store
            .routes_with_samples()
            .into_iter()
            .filter_map(|(key, samples)| entry_for_route(key, samples, min))
            .collect()
    };

    Ok(Json(SignatureSamplesResponse {
        min_samples: min,
        routes,
    }))
}

pub fn router(store: Arc<TrainStore>) -> Router {
    Router::new()
        .route(SIGNATURE_SAMPLES_PATH, get(signature_samples))
        .with_state(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask_profiles::MaskCharacteristics;

    #[test]
    fn response_ends_with_marker_when_long_enough() {
        let c = MaskCharacteristics {
            target_latency_ms: 1.0,
            latency_jitter_ms: 1.0,
            observed_query_length_bytes: None,
            observed_request_length_bytes: Some(10),
            observed_response_length_bytes: Some(30),
            observed_response_ttfb_ms: None,
            target_request_entropy_bits: None,
            target_response_entropy_bits: Some(7.5),
            target_request_ones_ratio: Some(0.5),
            target_response_ones_ratio: Some(0.5),
            target_request_byte_histogram16: None,
            target_response_byte_histogram16: None,
        };
        let v = synth_response(30, &c);
        assert!(v.ends_with(RESPONSE_END_MARKER));
        assert_eq!(v.len(), 30);
        let t = synth_response_text(30, &c);
        assert!(t.ends_with("Hello World!#"));
    }
}
