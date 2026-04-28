//! Behavior tests for `/v1/sandbox-templates*` (T1.L).
//!
//! Coverage matches the brief:
//! 1. First boot seeds 4 built-ins; second boot does NOT duplicate them.
//! 2. POST → GET round-trip; PATCH; DELETE for a custom template.
//! 3. POST /validate ok → 200 `{ "valid": true }`; bad → 200 `{ "valid":
//!    false, "stderr": "..." }`.
//! 4. Deleting an attached template nulls the conversation FK.
//! 5. Conversation create with `sandbox_template_id` round-trips via API.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use harness_server::{build_router, TOKEN_HEADER};
use http_body_util::BodyExt;
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tower::ServiceExt;

use common::{rejecting_runner, TestApp, TOKEN};

async fn json_request(
    router: Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body_bytes = match body {
        Some(v) => serde_json::to_vec(&v).unwrap(),
        None => Vec::new(),
    };
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(TOKEN_HEADER, TOKEN)
        .header("content-type", "application/json")
        .body(Body::from(body_bytes))
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            panic!(
                "non-JSON response body: {:?}",
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    (status, v)
}

// 1a. First boot seeds the four built-ins.
#[tokio::test]
async fn first_boot_seeds_four_builtins() {
    let app = TestApp::boot().await;
    let r = build_router(app.state.clone());

    let (status, body) = json_request(r, "GET", "/v1/sandbox-templates", None).await;
    assert_eq!(status, StatusCode::OK);

    let templates = body.get("templates").and_then(|t| t.as_array()).unwrap();
    assert_eq!(templates.len(), 4, "expected four built-ins, got {body}");

    let ids: Vec<&str> = templates
        .iter()
        .filter_map(|t| t.get("id").and_then(|v| v.as_str()))
        .collect();
    for required in [
        "strict-readonly",
        "no-network",
        "network-only",
        "permissive-dev",
    ] {
        assert!(
            ids.contains(&required),
            "missing built-in {required}; got {ids:?}"
        );
    }
    for t in templates {
        assert_eq!(
            t.get("is_builtin").and_then(|v| v.as_bool()),
            Some(true),
            "every seeded template should be is_builtin=true: {t}"
        );
    }
}

// 1b. Second boot against the same DB does not duplicate built-ins.
#[tokio::test]
async fn second_boot_does_not_duplicate_builtins() {
    let app = TestApp::boot().await;

    // Re-run the bootstrap path against the same DB file.
    let db_path = {
        // Read the pool's path indirectly by listing what we already have:
        // the TestApp fixture stores the DB in its tempdir at "test.sqlite".
        app._tempdir.path().join("test.sqlite")
    };
    let app2 = TestApp::reopen(&db_path).await;

    let r = build_router(app2.state.clone());
    let (status, body) = json_request(r, "GET", "/v1/sandbox-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    let n = body
        .get("templates")
        .and_then(|t| t.as_array())
        .map(|a| a.len())
        .unwrap();
    assert_eq!(n, 4, "second boot must not duplicate built-ins; saw {n}");
}

// 2. POST → GET round-trip; PATCH; DELETE for a custom template.
#[tokio::test]
async fn custom_template_crud_round_trip() {
    let app = TestApp::boot().await;
    let r = || build_router(app.state.clone());

    // CREATE
    let (status, created) = json_request(
        r(),
        "POST",
        "/v1/sandbox-templates",
        Some(json!({
            "name": "deny-all-but-tmp",
            "description": "Read/write only under /tmp.",
            "profile": "(version 1)\n(deny default)\n(allow file-read* (subpath \"/tmp\"))\n",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "created body: {created}");
    let id = created
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_owned();
    assert_eq!(
        created.get("is_builtin").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        created.get("name").and_then(|v| v.as_str()),
        Some("deny-all-but-tmp")
    );

    // GET
    let (status, fetched) =
        json_request(r(), "GET", &format!("/v1/sandbox-templates/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fetched.get("id").and_then(|v| v.as_str()),
        Some(id.as_str())
    );

    // PATCH name + profile
    let (status, patched) = json_request(
        r(),
        "PATCH",
        &format!("/v1/sandbox-templates/{id}"),
        Some(json!({
            "name": "tmp-only-v2",
            "profile": "(version 1)\n(deny default)\n",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch body: {patched}");
    assert_eq!(
        patched.get("name").and_then(|v| v.as_str()),
        Some("tmp-only-v2")
    );
    assert_eq!(
        patched.get("profile").and_then(|v| v.as_str()),
        Some("(version 1)\n(deny default)\n")
    );

    // DELETE
    let (status, _) =
        json_request(r(), "DELETE", &format!("/v1/sandbox-templates/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // GET → 404
    let (status, _) = json_request(r(), "GET", &format!("/v1/sandbox-templates/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// 3a. Validate: good profile → 200 { valid: true }.
#[tokio::test]
async fn validate_good_profile_returns_valid_true() {
    let app = TestApp::boot().await;
    let r = build_router(app.state.clone());

    // The fixture's runner accepts every profile, so any built-in id works.
    let (status, body) = json_request(
        r,
        "POST",
        "/v1/sandbox-templates/strict-readonly/validate",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body, json!({ "valid": true }));
}

// 3b. Validate: bad profile → 200 { valid: false, stderr: "..." }.
#[tokio::test]
async fn validate_bad_profile_returns_200_with_stderr() {
    let app = TestApp::boot_with_runner(rejecting_runner("syntax error: unexpected token")).await;
    let r = build_router(app.state.clone());

    let (status, body) = json_request(
        r,
        "POST",
        "/v1/sandbox-templates/strict-readonly/validate",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body.get("valid").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        body.get("stderr").and_then(|v| v.as_str()),
        Some("syntax error: unexpected token")
    );
}

// 4. Deleting a sandbox template that's attached to a conversation nulls
//    the conversation's FK (ON DELETE SET NULL — no cascade).
#[tokio::test]
async fn deleting_attached_template_nulls_conversation_fk() {
    let app = TestApp::boot().await;
    let r = || build_router(app.state.clone());

    // 1. Create a custom template.
    let (status, tpl) = json_request(
        r(),
        "POST",
        "/v1/sandbox-templates",
        Some(json!({
            "name": "fk-test",
            "profile": "(version 1)\n(deny default)\n",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let tpl_id = tpl.get("id").and_then(|v| v.as_str()).unwrap().to_owned();

    // 2. Create a conversation referencing it.
    let (status, conv) = json_request(
        r(),
        "POST",
        "/v1/conversations",
        Some(json!({
            "provider_id": "claude",
            "model": "claude-sonnet-4-6",
            "title": "fk-test",
            "sandbox_template_id": tpl_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "conv body: {conv}");
    let conv_id = conv.get("id").and_then(|v| v.as_str()).unwrap().to_owned();
    assert_eq!(
        conv.get("sandbox_template_id").and_then(|v| v.as_str()),
        Some(tpl_id.as_str())
    );

    // 3. Delete the template.
    let (status, _) = json_request(
        r(),
        "DELETE",
        &format!("/v1/sandbox-templates/{tpl_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 4. Conversation still exists, FK is now null (no cascade).
    let (status, conv_after) =
        json_request(r(), "GET", &format!("/v1/conversations/{conv_id}"), None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "conversation must survive: {conv_after}"
    );
    assert!(
        conv_after
            .get("sandbox_template_id")
            .map_or(true, Value::is_null),
        "FK must be nulled, got {conv_after}"
    );
}

// 5. Conversation create with sandbox_template_id round-trips via API.
#[tokio::test]
async fn conversation_create_round_trips_sandbox_template_id() {
    let app = TestApp::boot().await;
    let r = || build_router(app.state.clone());

    // Use one of the seeded built-ins.
    let (status, conv) = json_request(
        r(),
        "POST",
        "/v1/conversations",
        Some(json!({
            "provider_id": "claude",
            "model": "claude-sonnet-4-6",
            "title": "with-sandbox",
            "sandbox_template_id": "strict-readonly",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "conv body: {conv}");
    let conv_id = conv.get("id").and_then(|v| v.as_str()).unwrap().to_owned();
    assert_eq!(
        conv.get("sandbox_template_id").and_then(|v| v.as_str()),
        Some("strict-readonly")
    );

    // GET the conversation back and confirm the FK is preserved.
    let (status, fetched) =
        json_request(r(), "GET", &format!("/v1/conversations/{conv_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fetched.get("sandbox_template_id").and_then(|v| v.as_str()),
        Some("strict-readonly")
    );

    // PATCH to clear it (explicit null).
    let (status, patched) = json_request(
        r(),
        "PATCH",
        &format!("/v1/conversations/{conv_id}"),
        Some(json!({ "sandbox_template_id": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "patch body: {patched}");
    assert!(
        patched
            .get("sandbox_template_id")
            .map_or(true, Value::is_null),
        "explicit null must clear the FK, got {patched}"
    );

    // PATCH to attach a different built-in.
    let (status, patched) = json_request(
        r(),
        "PATCH",
        &format!("/v1/conversations/{conv_id}"),
        Some(json!({ "sandbox_template_id": "no-network" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        patched.get("sandbox_template_id").and_then(|v| v.as_str()),
        Some("no-network")
    );
}

// Built-ins are immutable: PATCH and DELETE return 409.
#[tokio::test]
async fn cannot_patch_or_delete_builtin_template() {
    let app = TestApp::boot().await;
    let r = || build_router(app.state.clone());

    let (status, _) = json_request(
        r(),
        "PATCH",
        "/v1/sandbox-templates/strict-readonly",
        Some(json!({ "name": "renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) =
        json_request(r(), "DELETE", "/v1/sandbox-templates/strict-readonly", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

// POST with empty name / profile → 400.
#[tokio::test]
async fn create_validates_required_fields() {
    let app = TestApp::boot().await;
    let r = build_router(app.state.clone());

    let (status, _) = json_request(
        r,
        "POST",
        "/v1/sandbox-templates",
        Some(json!({ "name": "", "profile": "(version 1)" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
