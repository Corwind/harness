//! `harness-sandbox` — built-in SBPL templates and adapter scaffolding for
//! wrapping subprocess tool invocations in macOS `sandbox-exec`.
//!
//! Phase 0 / track D scope: profiles, types, and validation tests only.
//! The runtime [`SandboxRunner`] adapter (which materialises profiles to a
//! tempdir and builds `sandbox-exec -f <profile> -- ...` commands) lands
//! in T1.K.
//!
//! The canonical `SandboxRunner` port, the `SandboxTemplate` entity, and
//! the `SandboxError` taxonomy live in `harness-core` (per PLAN §2.5).
//! This crate re-exports them for the convenience of callers that already
//! depend on `harness-sandbox`.
//!
//! Built-in profiles live under `./profiles/` and are embedded into the
//! binary at compile time via [`include_str!`].
//!
//! See `spec/sandbox.md` for the full narrative spec.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

mod runner;
mod wrap;

pub use harness_core::{SandboxError, SandboxRunner, SandboxTemplate, SandboxTemplateId};
pub use runner::SbxRunner;
pub use wrap::wrapped_command_to_tokio;

/// Stable identifier for the `strict-readonly` built-in template.
pub const BUILTIN_STRICT_READONLY: &str = "strict-readonly";
/// Stable identifier for the `no-network` built-in template.
pub const BUILTIN_NO_NETWORK: &str = "no-network";
/// Stable identifier for the `network-only` built-in template.
pub const BUILTIN_NETWORK_ONLY: &str = "network-only";
/// Stable identifier for the `permissive-dev` built-in template.
pub const BUILTIN_PERMISSIVE_DEV: &str = "permissive-dev";

const PROFILE_STRICT_READONLY: &str = include_str!("../profiles/strict-readonly.sb");
const PROFILE_NO_NETWORK: &str = include_str!("../profiles/no-network.sb");
const PROFILE_NETWORK_ONLY: &str = include_str!("../profiles/network-only.sb");
const PROFILE_PERMISSIVE_DEV: &str = include_str!("../profiles/permissive-dev.sb");

/// Returns the four built-in [`SandboxTemplate`]s in a stable order:
/// `strict-readonly`, `no-network`, `network-only`, `permissive-dev`.
///
/// Each template's `profile` is loaded from
/// `backend/crates/harness-sandbox/profiles/<id>.sb` at compile time. The
/// orchestrator / `SandboxTemplateRepo` is expected to seed these into
/// the DB on first run with `is_builtin = true`.
pub fn builtin_templates() -> Vec<SandboxTemplate> {
    vec![
        SandboxTemplate {
            id: SandboxTemplateId::from_string(BUILTIN_STRICT_READONLY),
            name: "Strict read-only".to_owned(),
            description: Some(
                "Deny default; read system paths, no writes, no network. \
                 Use for linters, parsers, analysers."
                    .to_owned(),
            ),
            profile: PROFILE_STRICT_READONLY.to_owned(),
            is_builtin: true,
        },
        SandboxTemplate {
            id: SandboxTemplateId::from_string(BUILTIN_NO_NETWORK),
            name: "No network".to_owned(),
            description: Some(
                "Allow default; deny all network. Use for tools that mutate \
                 user files but should not reach the network."
                    .to_owned(),
            ),
            profile: PROFILE_NO_NETWORK.to_owned(),
            is_builtin: true,
        },
        SandboxTemplate {
            id: SandboxTemplateId::from_string(BUILTIN_NETWORK_ONLY),
            name: "Network only".to_owned(),
            description: Some(
                "Deny default; allow network and system reads, no writes. \
                 Use for fetchers (curl, wget)."
                    .to_owned(),
            ),
            profile: PROFILE_NETWORK_ONLY.to_owned(),
            is_builtin: true,
        },
        SandboxTemplate {
            id: SandboxTemplateId::from_string(BUILTIN_PERMISSIVE_DEV),
            name: "Permissive (dev only)".to_owned(),
            description: Some(
                "Allow default with explicit denies for ~/.ssh, ~/.aws, \
                 ~/.config/gh, ~/Library/Keychains. Dev only — never ship as default."
                    .to_owned(),
            ),
            profile: PROFILE_PERMISSIVE_DEV.to_owned(),
            is_builtin: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_returns_four_in_stable_order() {
        let ts = builtin_templates();
        assert_eq!(ts.len(), 4);
        assert_eq!(ts[0].id.as_str(), BUILTIN_STRICT_READONLY);
        assert_eq!(ts[1].id.as_str(), BUILTIN_NO_NETWORK);
        assert_eq!(ts[2].id.as_str(), BUILTIN_NETWORK_ONLY);
        assert_eq!(ts[3].id.as_str(), BUILTIN_PERMISSIVE_DEV);
    }

    #[test]
    fn every_builtin_is_marked_is_builtin() {
        for t in builtin_templates() {
            assert!(t.is_builtin, "{} must have is_builtin=true", t.id);
        }
    }

    #[test]
    fn every_builtin_profile_starts_with_version_1() {
        // Skip leading comment / blank lines, then assert the first
        // significant line is `(version 1)` exactly. sandbox-exec is
        // strict about this.
        for t in builtin_templates() {
            let first_significant = t
                .profile
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with(';'))
                .unwrap_or("");
            assert_eq!(
                first_significant, "(version 1)",
                "profile {} must start with (version 1)",
                t.id
            );
        }
    }
}
