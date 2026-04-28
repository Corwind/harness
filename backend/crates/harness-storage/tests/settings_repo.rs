//! Behavior tests for `SqliteSettingsRepo`.

use harness_core::repo::SettingsRepo;
use harness_storage::{Db, Secret, SqliteSettingsRepo};
use tempfile::NamedTempFile;

async fn open_db() -> (NamedTempFile, Db) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path(), Secret::from_bytes([2u8; 32]))
        .await
        .expect("open db");
    (tmp, db)
}

#[tokio::test]
async fn get_missing_returns_none() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSettingsRepo::new(db);
    assert!(repo.get("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn put_then_get_round_trips_arbitrary_json() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSettingsRepo::new(db);

    let value = serde_json::json!({
        "theme": "dark",
        "default_model": "claude-3-5-sonnet",
        "nested": [1, 2, {"k": "v"}],
    });
    repo.put("ui", value.clone()).await.unwrap();

    let got = repo.get("ui").await.unwrap();
    assert_eq!(got, Some(value));
}

#[tokio::test]
async fn put_replaces_existing_value() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSettingsRepo::new(db);

    repo.put("k", serde_json::json!(1)).await.unwrap();
    repo.put("k", serde_json::json!(2)).await.unwrap();
    assert_eq!(repo.get("k").await.unwrap(), Some(serde_json::json!(2)));
}

#[tokio::test]
async fn delete_removes_key_and_is_idempotent() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSettingsRepo::new(db);

    repo.put("k", serde_json::json!("v")).await.unwrap();
    repo.delete("k").await.unwrap();
    assert!(repo.get("k").await.unwrap().is_none());
    // Second delete is a no-op.
    repo.delete("k").await.unwrap();
}

#[tokio::test]
async fn all_returns_every_pair() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteSettingsRepo::new(db);

    repo.put("a", serde_json::json!(1)).await.unwrap();
    repo.put("b", serde_json::json!("two")).await.unwrap();
    repo.put("c", serde_json::json!([3])).await.unwrap();

    let mut all = repo.all().await.unwrap();
    all.sort_by(|x, y| x.0.cmp(&y.0));
    assert_eq!(
        all,
        vec![
            ("a".into(), serde_json::json!(1)),
            ("b".into(), serde_json::json!("two")),
            ("c".into(), serde_json::json!([3])),
        ]
    );
}
