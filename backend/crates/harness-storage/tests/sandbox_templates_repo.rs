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
        // Storage layer overwrites these; the sentinels are never observed.
        created_at: 0,
        updated_at: 0,
    }
}

#[tokio::test]
async fn create_get_round_trips() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);

    let saved = repo
        .create(tpl("custom-1", "Custom 1", false))
        .await
        .unwrap();
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
        created_at: saved.created_at,
        updated_at: saved.updated_at,
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
    let err = repo.update(tpl("ghost", "Ghost", false)).await.unwrap_err();
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
async fn create_stamps_timestamps_and_round_trips_them() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);

    let before = chrono::Utc::now().timestamp();
    let saved = repo.create(tpl("ts-1", "Timestamps", false)).await.unwrap();
    let after = chrono::Utc::now().timestamp();

    // Storage layer overwrites the sentinel `0` with `now()` on insert.
    assert!(
        saved.created_at >= before && saved.created_at <= after,
        "expected stamped created_at within [{before}, {after}]; got {}",
        saved.created_at
    );
    assert_eq!(
        saved.created_at, saved.updated_at,
        "fresh insert: created_at == updated_at"
    );

    let fetched = repo.get(&saved.id).await.unwrap();
    assert_eq!(fetched.created_at, saved.created_at);
    assert_eq!(fetched.updated_at, saved.updated_at);
}

#[tokio::test]
async fn update_bumps_updated_at_but_preserves_created_at() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);
    let saved = repo.create(tpl("ts-2", "BumpMe", false)).await.unwrap();

    // Sleep just past one second so the i64 timestamp can advance.
    // i64-second granularity means an immediate update could land in
    // the same second; the assertion uses `>=` to stay robust either
    // way.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let next = SandboxTemplate {
        profile: "(version 1)\n; v2".into(),
        ..saved.clone()
    };
    let after = repo.update(next).await.unwrap();

    assert_eq!(
        after.created_at, saved.created_at,
        "created_at must not change on update"
    );
    assert!(
        after.updated_at > saved.updated_at,
        "updated_at must advance after update; before={}, after={}",
        saved.updated_at,
        after.updated_at
    );
}

#[tokio::test]
async fn seed_builtins_preserves_created_at_across_reseeds() {
    // Re-seeding a built-in should bump updated_at but not rewrite
    // the original created_at.
    let (_tmp, db) = open_db().await;
    let repo = SqliteSandboxTemplateRepo::new(db);

    let v1 = vec![tpl("strict-readonly", "Strict", true)];
    repo.seed_builtins(&v1).await.unwrap();
    let first = repo
        .get(&harness_core::ids::SandboxTemplateId::from_string(
            "strict-readonly",
        ))
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    repo.seed_builtins(&v1).await.unwrap();
    let second = repo
        .get(&harness_core::ids::SandboxTemplateId::from_string(
            "strict-readonly",
        ))
        .await
        .unwrap();

    assert_eq!(
        second.created_at, first.created_at,
        "second seed must preserve created_at"
    );
    assert!(
        second.updated_at > first.updated_at,
        "second seed must bump updated_at"
    );
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
        created_at: 0,
        updated_at: 0,
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
