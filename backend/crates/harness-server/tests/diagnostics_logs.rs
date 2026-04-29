//! Behavior tests for `GET /v1/diagnostics/logs` (T3.5a).
//!
//! Coverage matches the brief:
//! 1. After the server has logged some lines, the endpoint returns
//!    them in order with monotonic seq.
//! 2. `after_seq=K` only returns lines with `seq > K`.
//! 3. The ring buffer cap evicts the oldest entries (oldest dropped).
//! 4. Stderr still receives every line — regression guard that the
//!    in-memory layer doesn't displace the stderr writer.
//!
//! These tests bypass `tracing_subscriber::registry().init()` (which is
//! process-global and conflicts with parallel `#[tokio::test]`s) and
//! drive the `LogRing` directly via `push_line` (case 1, 2, 3) plus a
//! tracing-dispatch local-scope test (case 4).

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use harness_server::{build_router, LogRing, LogRingLayer, TOKEN_HEADER};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

use common::{TestApp, TOKEN};

/// Helper: turn an in-memory `Vec<u8>` into a `MakeWriter` for a fmt
/// layer. The returned `Arc<Mutex<Vec<u8>>>` is what the test inspects.
struct VecWriter {
    sink: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for VecWriter {
    type Writer = SinkWriter;
    fn make_writer(&'a self) -> Self::Writer {
        SinkWriter {
            sink: self.sink.clone(),
        }
    }
}

struct SinkWriter {
    sink: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl std::io::Write for SinkWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sink.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn fetch_logs(app: &TestApp, after_seq: Option<u64>) -> (StatusCode, Value) {
    let r = build_router(app.state.clone());
    let path = match after_seq {
        Some(s) => format!("/v1/diagnostics/logs?after_seq={s}"),
        None => "/v1/diagnostics/logs".to_string(),
    };
    let req = Request::builder()
        .method("GET")
        .uri(&path)
        .header(TOKEN_HEADER, TOKEN)
        .body(Body::empty())
        .unwrap();
    let res = r.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    (status, v)
}

// ---------------------------------------------------------------------------
// 1. Endpoint returns pushed lines in order with monotonic seq.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn endpoint_returns_pushed_lines_in_order() {
    let app = TestApp::boot().await;
    app.state
        .logs
        .push_line("info", "test", "first line".into());
    app.state
        .logs
        .push_line("warn", "test", "second line".into());
    app.state
        .logs
        .push_line("error", "test", "third line".into());

    let (status, body) = fetch_logs(&app, None).await;
    assert_eq!(status, StatusCode::OK);

    let logs = body["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 3);

    let messages: Vec<&str> = logs
        .iter()
        .map(|l| l["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, vec!["first line", "second line", "third line"]);

    let levels: Vec<&str> = logs.iter().map(|l| l["level"].as_str().unwrap()).collect();
    assert_eq!(levels, vec!["info", "warn", "error"]);

    // seqs are strictly increasing.
    let seqs: Vec<u64> = logs.iter().map(|l| l["seq"].as_u64().unwrap()).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "seqs must be strictly increasing: {seqs:?}"
    );
    // next_seq matches the last log's seq.
    assert_eq!(body["next_seq"].as_u64().unwrap(), *seqs.last().unwrap());
}

// ---------------------------------------------------------------------------
// 2. `after_seq=K` only returns lines with seq > K.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn after_seq_filters_returned_lines() {
    let app = TestApp::boot().await;
    for i in 0..5 {
        app.state
            .logs
            .push_line("info", "test", format!("line {i}"));
    }

    // Grab everything to learn the seq space.
    let (_, all) = fetch_logs(&app, None).await;
    let all_seqs: Vec<u64> = all["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["seq"].as_u64().unwrap())
        .collect();
    assert_eq!(all_seqs.len(), 5);

    // Cut at the third element; next call should return only the last 2.
    let cut = all_seqs[2];
    let (status, body) = fetch_logs(&app, Some(cut)).await;
    assert_eq!(status, StatusCode::OK);
    let after = body["logs"].as_array().unwrap();
    assert_eq!(after.len(), 2);
    for l in after {
        assert!(l["seq"].as_u64().unwrap() > cut);
    }

    // After the latest seq → empty + cursor echoed.
    let (_, body_empty) = fetch_logs(&app, Some(*all_seqs.last().unwrap())).await;
    assert!(body_empty["logs"].as_array().unwrap().is_empty());
    assert_eq!(
        body_empty["next_seq"].as_u64().unwrap(),
        *all_seqs.last().unwrap(),
        "no new lines: cursor echoed"
    );
}

// ---------------------------------------------------------------------------
// 3. Ring cap evicts oldest entries.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn ring_buffer_cap_drops_oldest() {
    let app = TestApp::boot().await;
    // Push 6,000 short lines; the ring caps at 5,000.
    for i in 0..6_000u64 {
        app.state.logs.push_line("info", "test", format!("l{i}"));
    }
    let (status, body) = fetch_logs(&app, None).await;
    assert_eq!(status, StatusCode::OK);
    let logs = body["logs"].as_array().unwrap();
    assert!(
        logs.len() <= 5_000,
        "expected ≤ 5,000 retained; got {}",
        logs.len()
    );
    // The most recent should be present; the oldest should not.
    let messages: Vec<&str> = logs
        .iter()
        .map(|l| l["message"].as_str().unwrap())
        .collect();
    assert!(messages.contains(&"l5999"), "newest line must be retained");
    assert!(
        !messages.contains(&"l0"),
        "oldest line must have been evicted"
    );
}

// ---------------------------------------------------------------------------
// 4. Stderr writer still receives lines when paired with the ring layer.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn ring_layer_does_not_displace_stderr_writer() {
    // We stand up a *local* dispatcher (not global) so this test can
    // run in parallel with the other #[tokio::test]s. Both layers
    // (fmt-into-Vec and ring) are attached.
    let captured: Arc<std::sync::Mutex<Vec<u8>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = VecWriter {
        sink: captured.clone(),
    };
    let ring = Arc::new(LogRing::new());
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_target(false)
        .with_ansi(false);
    let ring_layer = LogRingLayer::new(ring.clone());
    let subscriber = tracing_subscriber::registry()
        .with(fmt_layer)
        .with(ring_layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("hello-stderr-and-ring");
    });

    // Settle: the fmt layer writes synchronously, but give a moment.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Both sinks must have received it.
    let stderr_bytes = captured.lock().unwrap().clone();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes).to_string();
    assert!(
        stderr_text.contains("hello-stderr-and-ring"),
        "stderr writer must still receive lines: captured={stderr_text:?}"
    );

    let (lines, _) = ring.since(0);
    assert!(
        lines
            .iter()
            .any(|l| l.message.contains("hello-stderr-and-ring")),
        "ring layer must also receive lines"
    );
}
