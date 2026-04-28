//! HTTP handlers for `/v1/sandbox-templates*`.
//!
//! Endpoints:
//! * `GET    /v1/sandbox-templates`             — list (built-ins + user)
//! * `POST   /v1/sandbox-templates`             — create user template
//! * `GET    /v1/sandbox-templates/{id}`        — fetch one
//! * `PATCH  /v1/sandbox-templates/{id}`        — partial update
//! * `DELETE /v1/sandbox-templates/{id}`        — delete (FK SET NULL on conversations)
//! * `POST   /v1/sandbox-templates/{id}/validate` — dry-run via `SandboxRunner::validate`
//!
//! Per the T1.L brief, `/validate` returns 200 with `{ "valid": true }` on
//! success and 200 with `{ "valid": false, "stderr": "..." }` on a failed
//! profile (the client wants the diagnostic — validation failure is not an
//! HTTP error).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use harness_core::{ids::SandboxTemplateId, sandbox::SandboxTemplate, SandboxError};
use uuid::Uuid;

use crate::{
    dto::{
        CreateSandboxTemplateRequest, PatchSandboxTemplateRequest, SandboxTemplateDto,
        SandboxTemplatesEnvelope, ValidateSandboxTemplateResponse,
    },
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    // axum 0.7 path syntax: `:id`. Curly-brace placeholders only work in
    // axum 0.8+.
    Router::new()
        .route("/v1/sandbox-templates", get(list).post(create))
        .route(
            "/v1/sandbox-templates/:id",
            get(get_one).patch(patch).delete(delete),
        )
        .route("/v1/sandbox-templates/:id/validate", post(validate))
}

async fn list(State(state): State<AppState>) -> Result<Json<SandboxTemplatesEnvelope>, ApiError> {
    let templates = state.sandbox_templates.list().await?;
    Ok(Json(SandboxTemplatesEnvelope {
        templates: templates
            .into_iter()
            .map(SandboxTemplateDto::from)
            .collect(),
    }))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateSandboxTemplateRequest>,
) -> Result<(StatusCode, Json<SandboxTemplateDto>), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "validation",
            "name must not be empty",
        ));
    }
    if body.profile.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "validation",
            "profile must not be empty",
        ));
    }

    let template = SandboxTemplate {
        id: SandboxTemplateId::from_string(Uuid::new_v4().to_string()),
        name: body.name,
        description: body.description,
        profile: body.profile,
        is_builtin: false,
        // Storage stamps `now()` for both on insert; sentinels here.
        created_at: 0,
        updated_at: 0,
    };
    let stored = state.sandbox_templates.create(template).await?;
    Ok((StatusCode::CREATED, Json(stored.into())))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SandboxTemplateDto>, ApiError> {
    let id = SandboxTemplateId::from_string(id);
    let template = state.sandbox_templates.get(&id).await?;
    Ok(Json(template.into()))
}

async fn patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PatchSandboxTemplateRequest>,
) -> Result<Json<SandboxTemplateDto>, ApiError> {
    let id = SandboxTemplateId::from_string(id);
    let mut existing = state.sandbox_templates.get(&id).await?;

    // Built-ins are immutable per OpenAPI; surface 409.
    if existing.is_builtin {
        return Err(ApiError::conflict("built-in templates cannot be modified"));
    }

    if let Some(name) = body.name {
        if name.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "validation",
                "name must not be empty",
            ));
        }
        existing.name = name;
    }
    if let Some(desc_outer) = body.description {
        existing.description = desc_outer;
    }
    if let Some(profile) = body.profile {
        if profile.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "validation",
                "profile must not be empty",
            ));
        }
        existing.profile = profile;
    }

    let updated = state.sandbox_templates.update(existing).await?;
    Ok(Json(updated.into()))
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = SandboxTemplateId::from_string(id);

    let existing = state.sandbox_templates.get(&id).await?;
    if existing.is_builtin {
        return Err(ApiError::conflict("built-in templates cannot be deleted"));
    }
    state.sandbox_templates.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn validate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ValidateSandboxTemplateResponse>, ApiError> {
    let id = SandboxTemplateId::from_string(id);
    let template = state.sandbox_templates.get(&id).await?;

    match state.sandbox_runner.validate(&template.profile).await {
        Ok(()) => Ok(Json(ValidateSandboxTemplateResponse {
            valid: true,
            stderr: None,
        })),
        // Per the T1.L brief: malformed profile → 200 with valid=false +
        // stderr. The client (UI) needs the diagnostic to render inline.
        Err(SandboxError::ProfileInvalid { stderr }) => Ok(Json(ValidateSandboxTemplateResponse {
            valid: false,
            stderr: Some(stderr),
        })),
        Err(other) => Err(other.into()),
    }
}
