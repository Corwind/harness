//! Route modules.
//!
//! T1.A shipped `health`. T1.L added `sandbox_templates` and the
//! conversation create/get/patch verbs. T1.E completes the surface:
//! `providers`, `messages`, `runs` (with SSE), `settings`, plus
//! `conversations` list/delete.

pub mod conversations;
pub mod health;
pub mod messages;
pub mod providers;
pub mod runs;
pub mod sandbox_templates;
pub mod settings;
