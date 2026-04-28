//! Behavior tests for `SqliteConversationRepo`.
//!
//! Exercises only the public `ConversationRepo` trait surface from
//! `harness-core::repo` plus the `Db::open` constructor — never private
//! types of the adapter.

use harness_core::ids::{ConversationId, ProviderId, SandboxTemplateId};
use harness_core::repo::{
    ConversationPatch, ConversationRepo, NewConversation, SandboxTemplateRepo,
};
use harness_core::sandbox::SandboxTemplate;
use harness_core::RepoError;
use harness_storage::{Db, Secret, SqliteConversationRepo, SqliteSandboxTemplateRepo};
use tempfile::NamedTempFile;

async fn open_db() -> (NamedTempFile, Db) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path(), Secret::from_bytes([0u8; 32]))
        .await
        .expect("open db");
    (tmp, db)
}

fn new_conv(provider: &str, model: &str) -> NewConversation {
    NewConversation {
        title: "Test conversation".into(),
        provider_id: ProviderId::from_string(provider),
        model: model.into(),
        sandbox_template_id: None,
    }
}

#[tokio::test]
async fn create_and_get_round_trips() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteConversationRepo::new(db);

    let created = repo
        .create(new_conv("claude", "claude-3-5-sonnet"))
        .await
        .expect("create");

    assert_eq!(created.title, "Test conversation");
    assert_eq!(created.provider_id.as_str(), "claude");
    assert_eq!(created.model, "claude-3-5-sonnet");
    assert_eq!(created.sandbox_template_id, None);

    let fetched = repo.get(&created.id).await.expect("get");
    assert_eq!(fetched, created);
}

#[tokio::test]
async fn get_missing_returns_not_found() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteConversationRepo::new(db);

    let err = repo
        .get(&ConversationId::from_string("does-not-exist"))
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::NotFound), "got {err:?}");
}

#[tokio::test]
async fn list_returns_inserted_rows_newest_first() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteConversationRepo::new(db);

    let a = repo.create(new_conv("claude", "m1")).await.unwrap();
    // Sleep to guarantee distinct timestamps even on fast machines.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let b = repo.create(new_conv("claude", "m2")).await.unwrap();

    let all = repo.list().await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, b.id, "newest conversation should sort first");
    assert_eq!(all[1].id, a.id);
}

#[tokio::test]
async fn update_applies_only_present_fields() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteConversationRepo::new(db);

    let c = repo.create(new_conv("claude", "m1")).await.unwrap();

    let patched = repo
        .update(
            &c.id,
            ConversationPatch {
                title: Some("Renamed".into()),
                model: None,
                sandbox_template_id: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(patched.title, "Renamed");
    assert_eq!(patched.model, "m1", "model should be untouched");
    assert!(patched.updated_at >= c.updated_at);
}

#[tokio::test]
async fn update_can_set_and_clear_sandbox_template() {
    let (_tmp, db) = open_db().await;
    let conv_repo = SqliteConversationRepo::new(db.clone());
    let sb_repo = SqliteSandboxTemplateRepo::new(db);

    let tpl = sb_repo
        .create(SandboxTemplate {
            id: SandboxTemplateId::from_string("test-tpl"),
            name: "Test".into(),
            description: None,
            profile: "(version 1)".into(),
            is_builtin: false,
            created_at: 0,
            updated_at: 0,
        })
        .await
        .unwrap();

    let c = conv_repo.create(new_conv("claude", "m1")).await.unwrap();

    // Set the template.
    let patched = conv_repo
        .update(
            &c.id,
            ConversationPatch {
                title: None,
                model: None,
                sandbox_template_id: Some(Some(tpl.id.clone())),
            },
        )
        .await
        .unwrap();
    assert_eq!(patched.sandbox_template_id.as_ref(), Some(&tpl.id));

    // Clear the template (Some(None)).
    let cleared = conv_repo
        .update(
            &c.id,
            ConversationPatch {
                title: None,
                model: None,
                sandbox_template_id: Some(None),
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.sandbox_template_id, None);
}

#[tokio::test]
async fn create_with_unknown_sandbox_template_yields_conflict() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteConversationRepo::new(db);

    let mut nc = new_conv("claude", "m1");
    nc.sandbox_template_id = Some(SandboxTemplateId::from_string("missing"));
    let err = repo.create(nc).await.unwrap_err();
    assert!(
        matches!(err, RepoError::Conflict(_)),
        "expected Conflict, got {err:?}"
    );
}

#[tokio::test]
async fn delete_removes_row() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteConversationRepo::new(db);

    let c = repo.create(new_conv("claude", "m1")).await.unwrap();
    repo.delete(&c.id).await.unwrap();

    let err = repo.get(&c.id).await.unwrap_err();
    assert!(matches!(err, RepoError::NotFound));
}

#[tokio::test]
async fn deleting_sandbox_template_nulls_conversation_fk() {
    let (_tmp, db) = open_db().await;
    let conv_repo = SqliteConversationRepo::new(db.clone());
    let sb_repo = SqliteSandboxTemplateRepo::new(db);

    let tpl = sb_repo
        .create(SandboxTemplate {
            id: SandboxTemplateId::from_string("doomed"),
            name: "Doomed".into(),
            description: None,
            profile: "(version 1)".into(),
            is_builtin: false,
            created_at: 0,
            updated_at: 0,
        })
        .await
        .unwrap();

    let mut nc = new_conv("claude", "m1");
    nc.sandbox_template_id = Some(tpl.id.clone());
    let c = conv_repo.create(nc).await.unwrap();
    assert_eq!(c.sandbox_template_id.as_ref(), Some(&tpl.id));

    sb_repo.delete(&tpl.id).await.unwrap();

    let after = conv_repo.get(&c.id).await.unwrap();
    assert_eq!(
        after.sandbox_template_id, None,
        "FK should be set to NULL on sandbox template delete"
    );
}
