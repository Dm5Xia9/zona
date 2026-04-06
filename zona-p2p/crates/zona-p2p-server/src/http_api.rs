//! HTTP API for client applications to send packets and await responses.
//!
//! Single endpoint:
//!   POST /api/packet
//!     Body:    { "to_id": "<hex>", "payload": "<base64>", "timeout_ms": 30000 }
//!     Returns: { "payload": "<base64>" }          (200)
//!              { "error": "timeout" }             (408)
//!              { "error": "<message>" }           (400/500)

use std::time::Duration;

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json,
};
use serde::{Deserialize, Serialize};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

use zona_p2p_types::NodeId;

use crate::server::{self, Shared};

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PacketReq {
    /// Destination node ID (full hex, 64 chars).
    pub to_id:      String,
    /// Payload bytes encoded as standard base64.
    pub payload:    String,
    /// Max wait time in milliseconds (default: 30 000).
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 { 30_000 }

#[derive(Serialize)]
pub struct PacketOk {
    /// Response payload bytes encoded as standard base64.
    pub payload: String,
}

#[derive(Serialize)]
pub struct PacketErr {
    pub error: String,
}

// ── Handler ───────────────────────────────────────────────────────────────────

pub(crate) async fn handle_packet(
    State(state): State<Shared>,
    Json(req):    Json<PacketReq>,
) -> impl IntoResponse {
    // Decode destination NodeId.
    let to_bytes = match hex::decode(&req.to_id) {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("bad to_id: {e}") }))).into_response();
        }
    };
    let to_arr: [u8; 32] = match to_bytes.try_into() {
        Ok(a) => a,
        Err(_) => {
            return (StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "to_id must be 64 hex chars (32 bytes)" }))).into_response();
        }
    };
    let to_id = NodeId::from_bytes(to_arr);

    // Decode payload.
    let payload = match B64.decode(&req.payload) {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("bad payload base64: {e}") }))).into_response();
        }
    };

    let timeout = Duration::from_millis(req.timeout_ms);

    // Send packet and wait for response.
    match server::send_and_wait(state, to_id, payload, timeout).await {
        Ok(resp) => {
            let encoded = B64.encode(&resp);
            (StatusCode::OK, Json(serde_json::json!({ "payload": encoded }))).into_response()
        }
        Err(e) if e.to_string().contains("timeout") => {
            (StatusCode::REQUEST_TIMEOUT,
             Json(serde_json::json!({ "error": "timeout" }))).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR,
             Json(serde_json::json!({ "error": e.to_string() }))).into_response()
        }
    }
}

// ── Router factory ────────────────────────────────────────────────────────────

/// Build the axum router for the HTTP client API.
///
/// Usage in a binary:
/// ```rust,ignore
/// let router = http_api::make_router(server.shared());
/// let listener = tokio::net::TcpListener::bind(http_addr).await?;
/// axum::serve(listener, router).await?;
/// ```
pub fn make_router(state: Shared) -> Router {
    Router::new()
        .route("/api/packet", post(handle_packet))
        .with_state(state)
}
