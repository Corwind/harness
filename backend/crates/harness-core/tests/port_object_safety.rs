//! Behavior tests that exercise the storage/sandbox/secrets ports
//! through trait objects.
//!
//! The point isn't to test storage logic — that's the adapter's job —
//! but to lock in that:
//!   * each port is object-safe (`dyn Trait` works), so the
//!     orchestrator and HTTP layer can hold them behind `Arc<dyn _>`,
//!   * the trait surface is sufficient for the basic CRUD and
//!     wrap/validate flows the orchestrator will rely on.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use harness_core::error::{RepoError, SandboxError, SecretsError, ToolError};

use harness_core::ids::{ConversationId, ProviderId, SandboxTemplateId};
use harness_core::repo::{
    Conversation, ConversationPatch, ConversationRepo, NewConversation,
};
use harness_core::sandbox::{SandboxRunner, SandboxTemplate, WrappedCommand};
use harness_core::secrets::SecretsVault;
use harness_core::tool::ToolCommand;
use pretty_assertions::assert_eq;

// ---------------------------------------------------------------------------
// In-memory ConversationRepo

struct MemConversationRepo {
    rows: Mutex<HashMap<ConversationId, Conversation>>,
}

impl MemConversationRepo {
    fn new() -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ConversationRepo for MemConversationRepo {
    async fn create(&self, new: NewConversation) -> Result<Conversation, RepoError> {
        let now = Utc::now();
        let id = ConversationId::generate();
        let conv = Conversation {
            id: id.clone(),
            title: new.title,
            provider_id: new.provider_id,
            model: new.model,
            sandbox_template_id: new.sandbox_template_id,
            created_at: now,
            updated_at: now,
        };
        self.rows.lock().unwrap().insert(id, conv.clone());
        Ok(conv)
    }

    async fn get(&self, id: &ConversationId) -> Result<Conversation, RepoError> {
        self.rows
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(RepoError::NotFound)
    }

    async fn list(&self) -> Result<Vec<Conversation>, RepoError> {
        Ok(self.rows.lock().unwrap().values().cloned().collect())
    }

    async fn update(
        &self,
        id: &ConversationId,
        patch: ConversationPatch,
    ) -> Result<Conversation, RepoError> {
        let mut rows = self.rows.lock().unwrap();
        let row = rows.get_mut(id).ok_or(RepoError::NotFound)?;
        if let Some(t) = patch.title {
            row.title = t;
        }
        if let Some(m) = patch.model {
            row.model = m;
        }
        if let Some(s) = patch.sandbox_template_id {
            row.sandbox_template_id = s;
        }
        row.updated_at = Utc::now();
        Ok(row.clone())
    }

    async fn delete(&self, id: &ConversationId) -> Result<(), RepoError> {
        self.rows
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or(RepoError::NotFound)
    }
}

#[tokio::test]
async fn conversation_repo_is_object_safe_and_round_trips_through_dyn() {
    let repo: Arc<dyn ConversationRepo> = Arc::new(MemConversationRepo::new());

    let created = repo
        .create(NewConversation {
            title: "tea".into(),
            provider_id: ProviderId::from_string("claude"),
            model: "claude-sonnet-4-6".into(),
            sandbox_template_id: None,
        })
        .await
        .unwrap();

    let fetched = repo.get(&created.id).await.unwrap();
    assert_eq!(fetched, created);

    let patched = repo
        .update(
            &created.id,
            ConversationPatch {
                title: Some("coffee".into()),
                sandbox_template_id: Some(Some(SandboxTemplateId::from_string("tpl"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(patched.title, "coffee");
    assert_eq!(
        patched.sandbox_template_id,
        Some(SandboxTemplateId::from_string("tpl"))
    );

    let cleared = repo
        .update(
            &created.id,
            ConversationPatch {
                sandbox_template_id: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.sandbox_template_id, None);

    let listed = repo.list().await.unwrap();
    assert_eq!(listed.len(), 1);

    repo.delete(&created.id).await.unwrap();
    assert!(matches!(
        repo.get(&created.id).await.unwrap_err(),
        RepoError::NotFound
    ));
}

// ---------------------------------------------------------------------------
// Fake SandboxRunner

struct FakeSandbox;

#[async_trait]
impl SandboxRunner for FakeSandbox {
    async fn wrap(
        &self,
        template: &SandboxTemplate,
        cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError> {
        if template.profile.is_empty() {
            return Err(SandboxError::InvalidProfile("empty".into()));
        }
        let mut args = vec!["-p".to_string(), template.profile.clone(), "--".into(), cmd.program];
        args.extend(cmd.args);
        Ok(WrappedCommand {
            program: "/usr/bin/sandbox-exec".into(),
            args,
            env: cmd.env,
            cwd: cmd.cwd,
        })
    }

    async fn validate(&self, profile: &str) -> Result<(), SandboxError> {
        if profile.contains("(version 1)") {
            Ok(())
        } else {
            Err(SandboxError::ProfileInvalid {
                stderr: "missing version directive".into(),
            })
        }
    }
}

#[tokio::test]
async fn sandbox_runner_object_safe_and_wraps_command() {
    let runner: Arc<dyn SandboxRunner> = Arc::new(FakeSandbox);
    let tpl = SandboxTemplate {
        id: SandboxTemplateId::from_string("tpl"),
        name: "n".into(),
        description: None,
        profile: "(version 1)\n(deny default)".into(),
        is_builtin: false,
    };
    let wrapped = runner
        .wrap(
            &tpl,
            ToolCommand {
                program: "/bin/echo".into(),
                args: vec!["hi".into()],
                env: HashMap::new(),
                cwd: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(wrapped.program, "/usr/bin/sandbox-exec");
    assert!(wrapped.args.contains(&"/bin/echo".to_string()));
    assert!(wrapped.args.contains(&"hi".to_string()));

    runner.validate("(version 1) ...").await.unwrap();
    assert!(matches!(
        runner.validate("nope").await.unwrap_err(),
        SandboxError::ProfileInvalid { .. }
    ));
}

// ---------------------------------------------------------------------------
// Fake SecretsVault

struct MemVault {
    rows: Mutex<HashMap<String, String>>,
}

#[async_trait]
impl SecretsVault for MemVault {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretsError> {
        Ok(self.rows.lock().unwrap().get(key).cloned())
    }
    async fn put(&self, key: &str, value: &str) -> Result<(), SecretsError> {
        self.rows.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    async fn delete(&self, key: &str) -> Result<(), SecretsError> {
        self.rows.lock().unwrap().remove(key);
        Ok(())
    }
}

#[tokio::test]
async fn secrets_vault_object_safe_and_round_trips() {
    let vault: Arc<dyn SecretsVault> = Arc::new(MemVault {
        rows: Mutex::new(HashMap::new()),
    });
    assert_eq!(vault.get("k").await.unwrap(), None);
    vault.put("k", "v").await.unwrap();
    assert_eq!(vault.get("k").await.unwrap(), Some("v".into()));
    vault.delete("k").await.unwrap();
    assert_eq!(vault.get("k").await.unwrap(), None);
    // Delete is idempotent.
    vault.delete("k").await.unwrap();
}

// ---------------------------------------------------------------------------
// ToolError variants observable

#[test]
fn tool_error_no_sandbox_renders_explanatory_message() {
    let e = ToolError::NoSandbox {
        tool: "echo".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("echo"), "got: {s}");
    assert!(s.contains("no sandbox"), "got: {s}");
}
