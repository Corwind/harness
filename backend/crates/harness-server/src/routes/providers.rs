//! HTTP handlers for `/v1/providers*`.
//!
//! Three endpoints:
//! * `GET  /v1/providers`              — list registered providers + caps
//! * `GET  /v1/providers/:id/models`   — models exposed by a configured provider
//! * `POST /v1/providers/:id/config`   — upsert config (encrypted at rest)
//!
//! `configured` is computed by checking the `ProvidersConfigRepo` for a row
//! matching the provider's id. `list_models` returns 404 if the id is not
//! registered, 409 if it is registered but unconfigured.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use harness_core::{ids::ProviderId, provider::ProviderConfig};

use crate::{
    dto::{
        ModelDto, ModelsEnvelope, ProviderCapabilitiesDto, ProviderConfigSummaryDto, ProviderDto,
        ProvidersEnvelope, UpsertProviderConfigRequest,
    },
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/providers", get(list))
        .route("/v1/providers/:id/models", get(list_models))
        .route("/v1/providers/:id/config", post(upsert_config))
}

async fn list(State(state): State<AppState>) -> Result<Json<ProvidersEnvelope>, ApiError> {
    let providers = state.providers.all();
    let mut out = Vec::with_capacity(providers.len());
    for p in providers {
        let id = ProviderId::from_string(p.id());
        let configured = state.providers_config.get(&id).await?.is_some();
        out.push(ProviderDto {
            id: p.id().to_owned(),
            display_name: p.display_name().to_owned(),
            configured,
            capabilities: ProviderCapabilitiesDto::from(p.capabilities()),
        });
    }
    Ok(Json(ProvidersEnvelope { providers: out }))
}

async fn list_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ModelsEnvelope>, ApiError> {
    let provider_id = ProviderId::from_string(id.clone());
    let provider = state
        .providers
        .get(&provider_id)
        .ok_or_else(|| ApiError::not_found(format!("provider '{id}' is not registered")))?;

    let stored = state.providers_config.get(&provider_id).await?;
    let Some(config_value) = stored else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "provider.unconfigured",
            format!("provider '{id}' has no stored configuration"),
        ));
    };

    let cfg = ProviderConfig {
        provider_id: provider_id.clone(),
        config: config_value,
    };

    // ProviderError → ApiError mapping preserves semantics: 401 for
    // bad credentials, 429 with Retry-After for rate limits, etc.
    // See `error::From<ProviderError> for ApiError`.
    let models = provider.list_models(&cfg).await?;

    Ok(Json(ModelsEnvelope {
        models: models.into_iter().map(ModelDto::from).collect(),
    }))
}

async fn upsert_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpsertProviderConfigRequest>,
) -> Result<Json<ProviderConfigSummaryDto>, ApiError> {
    let provider_id = ProviderId::from_string(id.clone());
    if state.providers.get(&provider_id).is_none() {
        return Err(ApiError::not_found(format!(
            "provider '{id}' is not registered"
        )));
    }

    state
        .providers_config
        .put(&provider_id, body.config)
        .await?;

    Ok(Json(ProviderConfigSummaryDto {
        provider_id: id,
        configured: true,
        updated_at: chrono::Utc::now(),
    }))
}
