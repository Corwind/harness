//! HTTP handlers for `/v1/diagnostics/*`.
//!
//! T3.5a scope: a single `GET /v1/diagnostics/logs?after_seq=N`
//! endpoint backed by the in-memory ring in
//! `harness_server::diagnostics::LogRing`. The Settings UI polls this
//! endpoint; SSE live-tail is intentionally deferred.
//!
//! Wire shape:
//!
//! ```json
//! {
//!   "logs": [
//!     { "seq": 1, "level": "info", "ts": "...", "target": "...", "message": "..." }
//!   ],
//!   "next_seq": 1
//! }
//! ```
//!
//! `next_seq` is the largest `seq` returned (or `after_seq` echoed back
//! when nothing matched), so polling clients can keep their cursor
//! stable across empty windows.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{diagnostics::LogLine, error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/diagnostics/logs", get(list_logs))
}

#[derive(Debug, Default, Deserialize)]
struct ListQuery {
    #[serde(default)]
    after_seq: Option<u64>,
}

#[derive(Debug, Serialize)]
struct LogsEnvelope {
    logs: Vec<LogLine>,
    next_seq: u64,
}

async fn list_logs(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<LogsEnvelope>, ApiError> {
    let after = q.after_seq.unwrap_or(0);
    let (logs, next_seq) = state.logs.since(after);
    Ok(Json(LogsEnvelope { logs, next_seq }))
}
