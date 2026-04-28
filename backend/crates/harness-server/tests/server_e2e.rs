//! End-to-end test: bind a real loopback listener, hit `/v1/health` over HTTP,
//! then trigger graceful shutdown and ensure the server task completes.

mod common;

use std::time::Duration;

use harness_server::{bind_loopback, build_router, serve};
use pretty_assertions::assert_eq;
use tokio::sync::oneshot;

use common::{TestApp, TOKEN};

#[tokio::test]
async fn health_round_trips_over_real_loopback_and_server_shuts_down() {
    let app = TestApp::boot().await;

    let bound = bind_loopback().await.expect("bind loopback");
    let addr = bound.local_addr;
    let router = build_router(app.state.clone());

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        serve(bound, router, async move {
            let _ = shutdown_rx.await;
        })
        .await
        .expect("serve")
    });

    // Give the server a brief moment to start accepting.
    tokio::time::sleep(Duration::from_millis(20)).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    // 1. Valid token → 200.
    let url = format!("http://{addr}/v1/health");
    let res = client
        .get(&url)
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .expect("request");
    assert_eq!(res.status().as_u16(), 200);

    // 2. Missing token → 401.
    let res = client.get(&url).send().await.expect("request");
    assert_eq!(res.status().as_u16(), 401);

    // 3. Trigger graceful shutdown and wait for the task to finish.
    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("server shutdown within 5s")
        .expect("server task did not panic");
}
