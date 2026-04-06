use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use zona::{
    app_with_store, TrainStore, MASK_PROFILES_PATH, TEACH_PATH, TRAIN_STORE_DUMP_PATH,
    TRAINING_STATUS_PATH,
};

#[tokio::test]
async fn route_key_groups_queries_under_same_path() {
    let store = Arc::new(TrainStore::new(2));
    let app = app_with_store(store.clone());
    for (q, ms) in [("b=2&a=1", 10u64), ("a=1&b=2", 20)] {
        let body = format!(
            "{{\"method\":\"GET\",\"path\":\"/api/items\",\"query\":\"{q}\",\"unixMs\":1712000000000,\"durationMs\":{ms},\"statusCode\":200}}"
        );
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(TEACH_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NO_CONTENT);
    }
    assert_eq!(store.counts_for("GET|/api/items"), 2);
    let samples = store.samples_for("GET|/api/items").expect("samples");
    assert_eq!(samples[0].query.as_deref(), samples[1].query.as_deref());
    assert_eq!(samples[0].query.as_deref(), Some("a=1&b=2"));
}

#[tokio::test]
async fn post_teach_returns_204() {
    let app = app_with_store(Arc::new(TrainStore::new(10)));
    let body = r#"{"method":"GET","path":"/api/items","query":"id=1","unixMs":1712000000000,"durationMs":12,"statusCode":200}"#;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(TEACH_PATH)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn mask_profiles_not_ready_until_min_samples() {
    let store = Arc::new(TrainStore::new(3));
    let app = app_with_store(store);
    let uri = format!("{MASK_PROFILES_PATH}?method=GET&path=/api/items");
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let routes = j["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 0);
}

#[tokio::test]
async fn mask_profiles_ready_with_five_variants() {
    let store = Arc::new(TrainStore::new(3));
    let app = app_with_store(store.clone());
    for (i, (ms, ttfb)) in [(10u64, 5u64), (20, 8), (35, 12)]
        .iter()
        .enumerate()
    {
        let body = format!(
            "{{\"method\":\"GET\",\"path\":\"/x\",\"query\":\"\",\"unixMs\":{},\"durationMs\":{},\"statusCode\":200,\"requestLength\":100,\"responseLength\":500,\"responseTtfbMs\":{}}}",
            1712000000000i64 + i as i64,
            ms,
            ttfb
        );
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(TEACH_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NO_CONTENT);
    }

    let uri = format!("{MASK_PROFILES_PATH}?method=GET&path=/x");
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let routes = j["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["ready"], true);
    assert_eq!(routes[0]["variants"].as_array().unwrap().len(), 5);
    let v0 = &routes[0]["variants"][0];
    assert!(v0["maskingPercent"].as_f64().unwrap() >= 0.0);
    assert!(v0["characteristics"]["targetLatencyMs"].is_number());
    assert!(v0["characteristics"]["observedResponseTtfbMs"].is_number());
}

#[tokio::test]
async fn mask_profiles_no_query_omits_routes_without_enough_samples() {
    let store = Arc::new(TrainStore::new(2));
    let app = app_with_store(store);
    for (path, i) in [("/a", 0i64), ("/b", 1i64)] {
        let body = format!(
            "{{\"method\":\"GET\",\"path\":\"{path}\",\"query\":\"\",\"unixMs\":{},\"durationMs\":15,\"statusCode\":200}}",
            1712000000300i64 + i
        );
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(TEACH_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NO_CONTENT);
    }

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(MASK_PROFILES_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let routes = j["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 0);
}

#[tokio::test]
async fn training_status_complete_after_total_threshold() {
    let store = Arc::new(TrainStore::new_for_testing(1, 2));
    let app = app_with_store(store);

    let r0 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(TRAINING_STATUS_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b0 = axum::body::to_bytes(r0.into_body(), usize::MAX)
        .await
        .unwrap();
    let j0: serde_json::Value = serde_json::from_slice(&b0).unwrap();
    assert_eq!(j0["trainingComplete"], false);

    for i in 0..2 {
        let body = format!(
            "{{\"method\":\"GET\",\"path\":\"/t\",\"query\":\"\",\"unixMs\":{},\"durationMs\":1,\"statusCode\":200}}",
            1712000000100i64 + i
        );
        let rr = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(TEACH_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rr.status(), StatusCode::NO_CONTENT);
    }

    let r1 = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(TRAINING_STATUS_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b1 = axum::body::to_bytes(r1.into_body(), usize::MAX)
        .await
        .unwrap();
    let j1: serde_json::Value = serde_json::from_slice(&b1).unwrap();
    assert_eq!(j1["trainingComplete"], true);
    assert_eq!(j1["totalSamples"], 2);
}

#[tokio::test]
async fn training_all_routes_complete_when_min_keys_ready() {
    let store = Arc::new(TrainStore::new_for_ready_routes_test(2, 2));
    let app = app_with_store(store);

    for (path, i) in [("/a", 0i64), ("/a", 1), ("/b", 2), ("/b", 3)] {
        let body = format!(
            "{{\"method\":\"GET\",\"path\":\"{path}\",\"query\":\"\",\"unixMs\":{},\"durationMs\":1,\"statusCode\":200}}",
            1712000000200i64 + i
        );
        let rr = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(TEACH_PATH)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rr.status(), StatusCode::NO_CONTENT);
    }

    let r = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(TRAINING_STATUS_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b = axum::body::to_bytes(r.into_body(), usize::MAX)
        .await
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(j["trainingComplete"], true);
    assert_eq!(j["routesMeetingMinSamples"], 2);
    assert_eq!(j["minReadyRoutes"], 2);
}

#[tokio::test]
async fn train_store_dump_returns_routes_json() {
    let store = Arc::new(TrainStore::new(2));
    let app = app_with_store(store);
    let body = r#"{"method":"GET","path":"/z","query":"","unixMs":1712000000000,"durationMs":9,"statusCode":204}"#;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(TEACH_PATH)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(TRAIN_STORE_DUMP_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(j["trainingComplete"], false);
    let routes = j["routes"].as_object().unwrap();
    assert!(routes.contains_key("GET|/z"));
}
