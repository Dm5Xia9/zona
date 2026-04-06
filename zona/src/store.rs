//! Накопление выборок teach; опционально — JSON на диске и флаг «обучение завершено».

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// Один наблюдённый цикл «запрос → ответ» с хоста прокси.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub duration_ms: u64,
    pub status_code: u16,
    pub unix_ms: i64,
    /// Каноническая query (без `?`), после разбора в Zona; ключ маршрута в store **без** query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub request_length: Option<u64>,
    pub response_length: Option<u64>,
    pub response_ttfb_ms: Option<u64>,
    pub request_entropy_bits: Option<f64>,
    pub request_ones_ratio: Option<f64>,
    pub request_byte_histogram16: Option<Vec<f64>>,
    pub response_entropy_bits: Option<f64>,
    pub response_ones_ratio: Option<f64>,
    pub response_byte_histogram16: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingDoneMode {
    /// Глобальная сумма сэмплов по **всем** ключам ≥ `total_threshold` (без учёта значимости по каждому маршруту).
    /// Явный режим для простых сценариев; при множестве разных URL это не отражает достаточность выборки по каждому params.
    Total,
    /// Не менее `min_ready_routes` различных ключей (`METHOD|path` без query) имеют по ≥ `min_samples` сэмплов.
    /// Вариации query учитываются внутри одного ключа как отдельные сэмплы (`Sample.query`).
    AllRoutes,
}

/// Снимок данных обучения (тот же формат, что `train_store.json` на диске).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTrainStore {
    #[serde(default = "persist_version")]
    v: u8,
    #[serde(default)]
    training_complete: bool,
    completed_at_unix_ms: Option<i64>,
    /// Совпадает с `ZONA_HOST_SESSION` от ASP.NET при последнем сохранении; смена — новый цикл обучения.
    #[serde(default)]
    host_session_id: Option<String>,
    #[serde(default)]
    routes: HashMap<String, Vec<Sample>>,
}

fn persist_version() -> u8 {
    1
}

impl Default for PersistedTrainStore {
    fn default() -> Self {
        Self {
            v: 1,
            training_complete: false,
            completed_at_unix_ms: None,
            host_session_id: None,
            routes: HashMap::new(),
        }
    }
}

pub struct TrainStore {
    inner: Mutex<PersistedTrainStore>,
    pub min_samples: usize,
    data_path: Option<PathBuf>,
    done_mode: TrainingDoneMode,
    total_threshold: usize,
    /// В режиме `AllRoutes`: сколько ключей должны набрать ≥ `min_samples`.
    pub min_ready_routes: usize,
}

fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl TrainStore {
    /// Только память, обучение никогда не завершается автоматически (`total_threshold = usize::MAX`).
    pub fn new(min_samples: usize) -> Self {
        Self::with_options(
            min_samples.max(1),
            None,
            TrainingDoneMode::Total,
            usize::MAX,
            1,
            PersistedTrainStore::default(),
        )
    }

    /// Для интеграционных тестов: обучение завершается после `total_threshold` сэмплов (режим Total).
    #[doc(hidden)]
    pub fn new_for_testing(min_samples: usize, total_threshold: usize) -> Self {
        Self::with_options(
            min_samples.max(1),
            None,
            TrainingDoneMode::Total,
            total_threshold.max(1),
            1,
            PersistedTrainStore::default(),
        )
    }

    /// Режим `AllRoutes`: завершение после `min_ready_routes` ключей с ≥ `min_samples` сэмплов.
    #[doc(hidden)]
    pub fn new_for_ready_routes_test(min_samples: usize, min_ready_routes: usize) -> Self {
        Self::with_options(
            min_samples.max(1),
            None,
            TrainingDoneMode::AllRoutes,
            usize::MAX,
            min_ready_routes.max(1),
            PersistedTrainStore::default(),
        )
    }

    fn with_options(
        min_samples: usize,
        data_path: Option<PathBuf>,
        done_mode: TrainingDoneMode,
        total_threshold: usize,
        min_ready_routes: usize,
        initial: PersistedTrainStore,
    ) -> Self {
        Self {
            inner: Mutex::new(initial),
            min_samples,
            data_path,
            done_mode,
            total_threshold,
            min_ready_routes,
        }
    }

    pub fn from_env() -> Self {
        let min_samples = env::var("ZONA_TEACH_MIN_SAMPLES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(10usize);

        let data_path = resolve_data_dir().map(PathBuf::from);

        let done_mode = match env::var("ZONA_TRAINING_DONE_MODE")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "total" => TrainingDoneMode::Total,
            "all_routes" | "allroutes" | "" => TrainingDoneMode::AllRoutes,
            _ => TrainingDoneMode::AllRoutes,
        };

        let default_total = min_samples.saturating_mul(20usize).max(200);
        let total_threshold = env::var("ZONA_TRAINING_COMPLETE_TOTAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or(default_total);

        let min_ready_routes = env::var("ZONA_TRAINING_MIN_READY_ROUTES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(3usize);

        let host_session = env::var("ZONA_HOST_SESSION").unwrap_or_default();

        let mut persisted = if let Some(ref p) = data_path {
            load_from_disk(p).unwrap_or_default()
        } else {
            PersistedTrainStore::default()
        };

        if let Some(ref p) = data_path {
            if should_reset_store_for_host_session(&persisted.host_session_id, &host_session) {
                info!(
                    target: "zona::store",
                    "ZONA_HOST_SESSION changed or new host bind — clearing store, training restarts"
                );
                let mut fresh = PersistedTrainStore::default();
                if !host_session.is_empty() {
                    fresh.host_session_id = Some(host_session.clone());
                }
                persisted = fresh;
                let _ = save_to_disk(p, &persisted);
            }

            if !persisted.training_complete
                && check_training_done(
                    &persisted,
                    done_mode,
                    min_samples,
                    total_threshold,
                    min_ready_routes,
                )
            {
                persisted.training_complete = true;
                persisted.completed_at_unix_ms = persisted.completed_at_unix_ms.or(Some(unix_ms_now()));
                let _ = save_to_disk(p, &persisted);
            }
        }

        if persisted.training_complete {
            info!(
                target: "zona::store",
                ?data_path,
                "loaded store: training already complete"
            );
        } else if data_path.is_some() {
            info!(
                target: "zona::store",
                ?data_path,
                ?done_mode,
                total_threshold,
                min_samples,
                min_ready_routes,
                routes = persisted.routes.len(),
                "loaded store"
            );
        }

        Self::with_options(
            min_samples,
            data_path,
            done_mode,
            total_threshold,
            min_ready_routes,
            persisted,
        )
    }

    pub fn training_complete(&self) -> bool {
        let g = self.inner.lock().expect("train store mutex");
        g.training_complete
    }

    pub fn completed_at_unix_ms(&self) -> Option<i64> {
        let g = self.inner.lock().expect("train store mutex");
        g.completed_at_unix_ms
    }

    pub fn total_sample_count(&self) -> usize {
        let g = self.inner.lock().expect("train store mutex");
        g.routes.values().map(|v| v.len()).sum()
    }

    pub fn route_count(&self) -> usize {
        let g = self.inner.lock().expect("train store mutex");
        g.routes.len()
    }

    /// Минимум сэмплов среди всех ключей (узкое место для режима `all_routes`). `None`, если маршрутов ещё нет.
    pub fn min_route_sample_count(&self) -> Option<usize> {
        let g = self.inner.lock().expect("train store mutex");
        g.routes.values().map(|v| v.len()).min()
    }

    pub fn done_mode(&self) -> TrainingDoneMode {
        self.done_mode
    }

    pub fn total_threshold(&self) -> usize {
        self.total_threshold
    }

    /// Число ключей, у которых уже ≥ `min_samples` сэмплов.
    pub fn routes_meeting_min_samples(&self) -> usize {
        let g = self.inner.lock().expect("train store mutex");
        g.routes
            .values()
            .filter(|v| v.len() >= self.min_samples)
            .count()
    }

    pub fn data_directory_display(&self) -> Option<String> {
        self.data_path.as_ref().map(|p| p.display().to_string())
    }

    /// Ключ маршрута: `METHOD|path` (path нормализован, **без** query — варианты query в `Sample.query`).
    pub fn route_key(method: &str, path: &str) -> String {
        let m = method.trim().to_ascii_uppercase();
        let p = normalize_route_path(path);
        format!("{m}|{p}")
    }

    pub fn record(&self, key: String, sample: Sample) {
        let path = self.data_path.clone();
        let done_mode = self.done_mode;
        let min_samples = self.min_samples;
        let total_threshold = self.total_threshold;
        let min_ready_routes = self.min_ready_routes;

        let mut g = self.inner.lock().expect("train store mutex");
        let was_complete = g.training_complete;
        g.routes.entry(key).or_default().push(sample);

        if !was_complete
            && check_training_done(
                &g,
                done_mode,
                min_samples,
                total_threshold,
                min_ready_routes,
            )
        {
            g.training_complete = true;
            g.completed_at_unix_ms = Some(unix_ms_now());
            info!(
                target: "zona::store",
                "training marked complete (mode {:?}, total samples {})",
                done_mode,
                g.routes.values().map(|v| v.len()).sum::<usize>()
            );
        }

        if let Some(ref p) = path {
            if let Err(e) = save_to_disk(p, &g) {
                error!(target: "zona::store", error = %e, "failed to persist train store");
            }
        }
    }

    pub fn counts_for(&self, key: &str) -> usize {
        let g = self.inner.lock().expect("train store mutex");
        g.routes.get(key).map(|v| v.len()).unwrap_or(0)
    }

    pub fn samples_for(&self, key: &str) -> Option<Vec<Sample>> {
        let g = self.inner.lock().expect("train store mutex");
        g.routes.get(key).map(|v| v.to_vec())
    }

    /// Все сэмплы по всем ключам (сортировка по `unix_ms`; для отладки, не для смешивания профилей).
    pub fn all_samples_pooled(&self) -> Vec<Sample> {
        let g = self.inner.lock().expect("train store mutex");
        let mut out: Vec<Sample> = g
            .routes
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();
        out.sort_by_key(|s| s.unix_ms);
        out
    }

    /// Каждый ключ `METHOD|path` со своей копией сэмплов; ключи отсортированы (стабильный JSON).
    pub fn routes_with_samples(&self) -> Vec<(String, Vec<Sample>)> {
        let g = self.inner.lock().expect("train store mutex");
        let mut pairs: Vec<(String, Vec<Sample>)> = g
            .routes
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs
    }

    /// Полный снимок для диагностики (GET `/train-store` и т.п.).
    pub fn persisted_snapshot(&self) -> PersistedTrainStore {
        let g = self.inner.lock().expect("train store mutex");
        g.clone()
    }
}

fn path_without_query_fragment(path: &str) -> &str {
    path.split_once('?').map(|(a, _)| a).unwrap_or(path)
}

fn normalize_route_path(path: &str) -> String {
    let path = path_without_query_fragment(path).trim();
    if path.is_empty() {
        return "/".to_string();
    }
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn check_training_done(
    state: &PersistedTrainStore,
    mode: TrainingDoneMode,
    min_samples: usize,
    total_threshold: usize,
    min_ready_routes: usize,
) -> bool {
    match mode {
        TrainingDoneMode::Total => {
            let n: usize = state.routes.values().map(|v| v.len()).sum();
            n >= total_threshold
        }
        TrainingDoneMode::AllRoutes => {
            let ready = state
                .routes
                .values()
                .filter(|v| v.len() >= min_samples)
                .count();
            ready >= min_ready_routes
        }
    }
}

/// Сброс только если хост явно передал непустую сессию и она не совпадает с сохранённой.
/// Пустой `ZONA_HOST_SESSION` (ручной запуск zona) — данные на диске не трогаем.
fn should_reset_store_for_host_session(persisted_id: &Option<String>, env_session: &str) -> bool {
    if env_session.is_empty() {
        return false;
    }
    persisted_id.as_deref() != Some(env_session)
}

fn resolve_data_dir() -> Option<String> {
    if let Ok(p) = env::var("ZONA_DATA_DIR") {
        let t = p.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

fn store_file_path(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("train_store.json")
}

fn load_from_disk(data_dir: &std::path::Path) -> io::Result<PersistedTrainStore> {
    let path = store_file_path(data_dir);
    if !path.exists() {
        return Ok(PersistedTrainStore::default());
    }
    let text = fs::read_to_string(&path)?;
    match serde_json::from_str::<PersistedTrainStore>(&text) {
        Ok(s) => Ok(s),
        Err(e) => {
            warn!(target: "zona::store", error = %e, "corrupt train_store.json, starting fresh");
            Ok(PersistedTrainStore::default())
        }
    }
}

fn save_to_disk(data_dir: &std::path::Path, state: &PersistedTrainStore) -> io::Result<()> {
    fs::create_dir_all(data_dir)?;
    let path = store_file_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    let json =
        serde_json::to_string_pretty(state).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}
