//! HTTP handlers for `/v1/settings`.
//!
//! `GET` returns the merged settings object (the union of all rows in
//! `SettingsRepo`). `PATCH` accepts a partial JSON object whose keys are
//! merged into the stored settings (top-level shallow merge: explicit
//! `null` clears a key, any other value overwrites). The wire shape is
//! free-form per OpenAPI.
//!
//! Storage layout: each top-level key in the wire shape is stored as a
//! separate row in the `settings` table. This keeps writes cheap and
//! lets us evolve the shape without migrations.

use axum::{extract::State, routing::get, Json, Router};

use crate::{error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/settings", get(get_all).patch(patch))
}

async fn get_all(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let entries = state.settings.all().await?;
    let mut obj = serde_json::Map::new();
    for (k, v) in entries {
        obj.insert(k, v);
    }
    Ok(Json(serde_json::Value::Object(obj)))
}

async fn patch(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let obj = body.as_object().ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "validation",
            "PATCH body must be a JSON object",
        )
    })?;
    for (k, v) in obj {
        if v.is_null() {
            state.settings.delete(k).await?;
        } else {
            state.settings.put(k, v.clone()).await?;
        }
    }
    // Return the merged settings.
    let entries = state.settings.all().await?;
    let mut merged = serde_json::Map::new();
    for (k, v) in entries {
        merged.insert(k, v);
    }
    Ok(Json(serde_json::Value::Object(merged)))
}
