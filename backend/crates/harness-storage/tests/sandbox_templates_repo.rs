//! Behavior tests for `SqliteSandboxTemplateRepo`.

use harness_core::ids::SandboxTemplateId;
use harness_core::repo::SandboxTemplateRepo;
use harness_core::sandbox::SandboxTemplate;
use harness_core::RepoError;
use harness_storage::{Db, Secret, SqliteSandboxTemplateRepo};
use tempfile::NamedTempFile;

async fn open_db() -> (NamedTempFile, Db) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path(), Secret::from_bytes([7u8; 32]))
        .await
        .expect("open db");
    (tmp, db)
}

fn tpl(id: &str, name: &str, builtin: bool) -> SandboxTemplate {
    SandboxTemplate {
        id: SandboxTemplateId::from_string(id),
        name: name.into(),
        description: Some(format!("desc for {name}")),
        profile: "(version 1)\n(deny default)".into(),
        is_builtin: builtin,
    }
}

#[tokio::test]
async fn create_get_round_trips() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);

    let saved = repo.create(tpl("custom-1", "Custom 1", false)).await.unwrap();
    let fetched = repo.get(&saved.id).await.unwrap();
    assert_eq!(fetched, saved);
}

#[tokio::test]
async fn create_with_duplicate_name_yields_conflict() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);
    repo.create(tpl("a", "Same Name", false)).await.unwrap();
    let err = repo.create(tpl("b", "Same Name", false)).await.unwrap_err();
    assert!(matches!(err, RepoError::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn list_returns_all() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);
    repo.create(tpl("a", "A", false)).await.unwrap();
    repo.create(tpl("b", "B", true)).await.unwrap();
    let mut all = repo.list().await.unwrap();
    all.sort_by(|x, y| x.id.as_str().cmp(y.id.as_str()));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id.as_str(), "a");
    assert_eq!(all[1].id.as_str(), "b");
}

#[tokio::test]
async fn update_overwrites_fields() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);
    let saved = repo.create(tpl("a", "Old", false)).await.unwrap();

    let updated = SandboxTemplate {
        name: "New".into(),
        description: None,
        profile: "(version 1)\n(allow default)".into(),
        is_builtin: saved.is_builtin,
        id: saved.id.clone(),
    };
    let after = repo.update(updated.clone()).await.unwrap();
    assert_eq!(after.name, "New");
    assert_eq!(after.description, None);
    assert_eq!(after.profile, "(version 1)\n(allow default)");

    let fetched = repo.get(&saved.id).await.unwrap();
    assert_eq!(fetched, after);
}

#[tokio::test]
async fn update_missing_yields_not_found() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);
    let err = repo
        .update(tpl("ghost", "Ghost", false))
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::NotFound), "got {err:?}");
}

#[tokio::test]
async fn delete_removes_row() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);
    let saved = repo.create(tpl("a", "A", false)).await.unwrap();
    repo.delete(&saved.id).await.unwrap();
    assert!(matches!(
        repo.get(&saved.id).await.unwrap_err(),
        RepoError::NotFound
    ));
}

#[tokio::test]
async fn seed_builtins_is_idempotent() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);

    let builtins = vec![
        tpl("strict-readonly", "Strict read-only", true),
        tpl("no-network", "No network", true),
    ];

    repo.seed_builtins(&builtins).await.unwrap();
    repo.seed_builtins(&builtins).await.unwrap();

    let all = repo.list().await.unwrap();
    assert_eq!(all.len(), 2, "seed must not duplicate rows");
    for t in &all {
        assert!(t.is_builtin);
    }
}

#[tokio::test]
async fn seed_builtins_updates_changed_profile() {
    // Re-seeding after a profile bump should refresh the stored profile
    // for an existing built-in (so we can ship updates to the bundled
    // SBPL without users losing the new content).
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);

    let v1 = SandboxTemplate {
        id: SandboxTemplateId::from_string("strict-readonly"),
        name: "Strict read-only".into(),
        description: None,
        profile: "(version 1)\n; v1".into(),
        is_builtin: true,
    };
    let v2 = SandboxTemplate {
        profile: "(version 1)\n; v2".into(),
        ..v1.clone()
    };

    repo.seed_builtins(std::slice::from_ref(&v1)).await.unwrap();
    repo.seed_builtins(std::slice::from_ref(&v2)).await.unwrap();

    let stored = repo.get(&v1.id).await.unwrap();
    assert_eq!(stored.profile, "(version 1)\n; v2");
}
