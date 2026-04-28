//! HTTP handlers for `/v1/conversations*`.
//!
//! T1.L scope: only the create + patch verbs needed to round-trip
//! `sandbox_template_id` (test #5 in the brief) plus a minimal `GET /{id}`
//! so tests can verify the persisted FK without reaching into the storage
//! crate. The full surface (`GET /v1/conversations`, `DELETE`,
//! `/messages/*`, `/runs/*`) lands in T1.E.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use harness_core::ids::ConversationId;

use crate::{
    dto::{ConversationDto, CreateConversationRequest, PatchConversationRequest},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    // axum 0.7 path syntax: `:id`.
    Router::new()
        .route("/v1/conversations", post(create))
        .route("/v1/conversations/:id", get(get_one).patch(patch))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateConversationRequest>,
) -> Result<(StatusCode, Json<ConversationDto>), ApiError> {
    if body.provider_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "validation",
            "provider_id must not be empty",
        ));
    }
    if body.model.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "validation",
            "model must not be empty",
        ));
    }

    let conv = state.conversations.create(body.into_new()).await?;
    Ok((StatusCode::CREATED, Json(conv.into())))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ConversationDto>, ApiError> {
    let id = ConversationId::from_string(id);
    let conv = state.conversations.get(&id).await?;
    Ok(Json(conv.into()))
}

async fn patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchConversationRequest>,
) -> Result<Json<ConversationDto>, ApiError> {
    let id = ConversationId::from_string(id);
    let conv = state.conversations.update(&id, body.into_patch()).await?;
    Ok(Json(conv.into()))
}
