//! Route modules.
//!
//! T1.A shipped `health`. T1.L adds `sandbox_templates` and a minimal
//! `conversations` (create/get/patch) needed to round-trip
//! `sandbox_template_id`. The remaining endpoints land in T1.E.

pub mod conversations;
pub mod health;
pub mod sandbox_templates;
