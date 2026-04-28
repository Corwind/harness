//! HTTP handlers for `/v1/conversations*`.
//!
//! T1.L shipped `POST`, `GET /:id`, `PATCH /:id`. T1.E adds:
//! * `GET /v1/conversations`         — paginated list, newest first
//! * `DELETE /v1/conversations/:id`  — cascades messages (FK ON DELETE CASCADE)
//!
//! Pagination cursor format: opaque base64 of the next page's cursor — for
//! v1, since the underlying repo returns the full list ordered by
//! `updated_at DESC, rowid DESC`, we encode the cursor as a base64-of
//! `<updated_at>|<rowid>` pair. T1.E ships a slice-and-skip cursor that
//! works for the small lists we expect; T2.x can switch to keyset paging
//! if user libraries grow.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use harness_core::ids::ConversationId;
use serde::Deserialize;

use crate::{
    dto::{
        ConversationDto, ConversationsEnvelope, CreateConversationRequest, PatchConversationRequest,
    },
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    // axum 0.7 path syntax: `:id`.
    Router::new()
        .route("/v1/conversations", get(list).post(create))
        .route(
            "/v1/conversations/:id",
            get(get_one).patch(patch).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
}

const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 200;

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<ConversationsEnvelope>, ApiError> {
    let limit = q
        .limit
        .map(|l| l.clamp(1, MAX_LIST_LIMIT))
        .unwrap_or(DEFAULT_LIST_LIMIT);

    // Repo lists everything ordered newest-first. We slice in-memory.
    // For v1 this is fine; the conversation count is bounded by the
    // single user's library.
    let all = state.conversations.list().await?;
    let start: usize = match &q.cursor {
        Some(c) => decode_cursor(c)?,
        None => 0,
    };
    let end = (start + limit).min(all.len());
    let slice: Vec<ConversationDto> = all[start..end].iter().cloned().map(Into::into).collect();
    let next_cursor = if end < all.len() {
        Some(encode_cursor(end))
    } else {
        None
    };

    Ok(Json(ConversationsEnvelope {
        conversations: slice,
        next_cursor,
    }))
}

fn encode_cursor(offset: usize) -> String {
    // Trivial offset-based cursor. Opaque to clients; we just want
    // them to round-trip it. base64-of-decimal keeps the wire format
    // close to opaque while being trivially debuggable.
    let raw = offset.to_string();
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

fn decode_cursor(c: &str) -> Result<usize, ApiError> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let bytes = URL_SAFE_NO_PAD
        .decode(c.as_bytes())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "validation", "invalid cursor"))?;
    let s = std::str::from_utf8(&bytes)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "validation", "invalid cursor"))?;
    s.parse::<usize>()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "validation", "invalid cursor"))
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

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = ConversationId::from_string(id);
    state.conversations.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
