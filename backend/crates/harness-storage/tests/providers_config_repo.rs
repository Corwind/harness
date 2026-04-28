//! Behavior tests for `SqliteProvidersConfigRepo`.
//!
//! Verifies that:
//!   * configs round-trip through encryption,
//!   * the on-disk blob is *not* the plaintext JSON,
//!   * a different key cannot decrypt the blob.

use harness_core::ids::ProviderId;
use harness_core::repo::ProvidersConfigRepo;
use harness_storage::{Db, Secret, SqliteProvidersConfigRepo};
use sqlx::Row;
use tempfile::NamedTempFile;

async fn open_db_with_key(path: &std::path::Path, key: [u8; 32]) -> Db {
    Db::open(path, Secret::from_bytes(key)).await.expect("open db")
}

#[tokio::test]
async fn put_then_get_round_trips_plaintext() {
    let tmp = NamedTempFile::new().unwrap();
    let db = open_db_with_key(tmp.path(), [9u8; 32]).await;
    let repo = SqliteProvidersConfigRepo::new(db);

    let cfg = serde_json::json!({"api_key": "sk-secret-abc", "base_url": null});
    repo.put(&ProviderId::from_string("claude"), cfg.clone())
        .await
        .unwrap();

    let got = repo
        .get(&ProviderId::from_string("claude"))
        .await
        .unwrap();
    assert_eq!(got, Some(cfg));
}

#[tokio::test]
async fn get_missing_returns_none() {
    let tmp = NamedTempFile::new().unwrap();
    let db = open_db_with_key(tmp.path(), [9u8; 32]).await;
    let repo = SqliteProvidersConfigRepo::new(db);
    assert!(repo
        .get(&ProviderId::from_string("nope"))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn stored_blob_is_not_plaintext() {
    let tmp = NamedTempFile::new().unwrap();
    let db = open_db_with_key(tmp.path(), [9u8; 32]).await;
    let repo = SqliteProvidersConfigRepo::new(db.clone());

    let secret = "sk-this-must-not-leak-9f5a";
    repo.put(
        &ProviderId::from_string("claude"),
        serde_json::json!({"api_key": secret}),
    )
    .await
    .unwrap();

    // Inspect the raw blob via the underlying pool.
    let row = sqlx::query("SELECT config_json FROM providers_config WHERE provider_id = 'claude'")
        .fetch_one(db.pool())
        .await
        .unwrap();
    let blob: Vec<u8> = row.try_get("config_json").unwrap();
    assert!(!blob.is_empty());
    let blob_str = String::from_utf8_lossy(&blob);
    assert!(
        !blob_str.contains(secret),
        "plaintext secret leaked into stored blob"
    );
    assert!(
        !blob_str.contains("api_key"),
        "plaintext field name leaked into stored blob"
    );
}

#[tokio::test]
async fn wrong_key_fails_to_decrypt() {
    let tmp = NamedTempFile::new().unwrap();
    {
        let db = open_db_with_key(tmp.path(), [1u8; 32]).await;
        let repo = SqliteProvidersConfigRepo::new(db);
        repo.put(
            &ProviderId::from_string("claude"),
            serde_json::json!({"api_key": "secret"}),
        )
        .await
        .unwrap();
    }
    // Reopen with a different key.
    let db2 = open_db_with_key(tmp.path(), [2u8; 32]).await;
    let repo2 = SqliteProvidersConfigRepo::new(db2);
    let err = repo2
        .get(&ProviderId::from_string("claude"))
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("decrypt") || msg.to_lowercase().contains("crypto"),
        "expected decrypt/crypto error, got: {msg}"
    );
}

#[tokio::test]
async fn put_replaces_existing_with_fresh_nonce() {
    // Two puts of the *same* plaintext must yield distinct ciphertexts
    // (random nonce per encryption). This is a property test, not a
    // correctness test, but it catches the "static nonce" footgun.
    let tmp = NamedTempFile::new().unwrap();
    let db = open_db_with_key(tmp.path(), [3u8; 32]).await;
    let repo = SqliteProvidersConfigRepo::new(db.clone());
    let id = ProviderId::from_string("claude");
    let cfg = serde_json::json!({"api_key": "constant"});

    repo.put(&id, cfg.clone()).await.unwrap();
    let blob1: Vec<u8> = sqlx::query("SELECT config_json FROM providers_config WHERE provider_id = 'claude'")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("config_json")
        .unwrap();

    repo.put(&id, cfg.clone()).await.unwrap();
    let blob2: Vec<u8> = sqlx::query("SELECT config_json FROM providers_config WHERE provider_id = 'claude'")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("config_json")
        .unwrap();

    assert_ne!(blob1, blob2, "ciphertext for identical plaintext must differ");
    // And both decrypt to the same plaintext.
    assert_eq!(repo.get(&id).await.unwrap(), Some(cfg));
}

#[tokio::test]
async fn delete_removes_entry() {
    let tmp = NamedTempFile::new().unwrap();
    let db = open_db_with_key(tmp.path(), [4u8; 32]).await;
    let repo = SqliteProvidersConfigRepo::new(db);
    let id = ProviderId::from_string("claude");

    repo.put(&id, serde_json::json!({"k": "v"})).await.unwrap();
    repo.delete(&id).await.unwrap();
    assert!(repo.get(&id).await.unwrap().is_none());
    repo.delete(&id).await.unwrap(); // idempotent
}

#[tokio::test]
async fn list_returns_all_decrypted() {
    let tmp = NamedTempFile::new().unwrap();
    let db = open_db_with_key(tmp.path(), [5u8; 32]).await;
    let repo = SqliteProvidersConfigRepo::new(db);

    repo.put(
        &ProviderId::from_string("claude"),
        serde_json::json!({"api_key": "a"}),
    )
    .await
    .unwrap();
    repo.put(
        &ProviderId::from_string("ollama"),
        serde_json::json!({"base_url": "http://localhost:11434"}),
    )
    .await
    .unwrap();

    let mut got = repo.list().await.unwrap();
    got.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0.as_str(), "claude");
    assert_eq!(got[0].1, serde_json::json!({"api_key": "a"}));
    assert_eq!(got[1].0.as_str(), "ollama");
}
