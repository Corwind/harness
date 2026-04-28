//! Behavior test for the initial storage migration.
//!
//! Asserts that `harness_storage::MIGRATOR` applies cleanly against a fresh
//! SQLite file, that every expected table and column exists with the right
//! type, and that re-applying the migrator is a no-op (idempotent).

use std::collections::BTreeMap;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use tempfile::NamedTempFile;

/// Connect to the SQLite file at `path`, creating it if necessary.
async fn connect(path: &std::path::Path) -> SqlitePool {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect sqlite")
}

/// Returns true iff a table with `name` exists in `sqlite_master`.
async fn table_exists(pool: &SqlitePool, name: &str) -> bool {
    let row = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .expect("query sqlite_master");
    row.is_some()
}

/// Returns true iff an index with `name` exists in `sqlite_master`.
async fn index_exists(pool: &SqlitePool, name: &str) -> bool {
    let row = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'index' AND name = ?1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .expect("query sqlite_master for index");
    row.is_some()
}

/// (column_name → declared_type) from `PRAGMA table_info(<table>)`.
async fn columns(pool: &SqlitePool, table: &str) -> BTreeMap<String, String> {
    // PRAGMA does not accept bind params; the table name is from a const list
    // controlled by this test, so string-formatting it here is safe.
    let sql = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .expect("pragma table_info");
    rows.into_iter()
        .map(|r| {
            let name: String = r.try_get("name").expect("col name");
            let ty: String = r.try_get("type").expect("col type");
            (name, ty.to_uppercase())
        })
        .collect()
}

fn assert_column(cols: &BTreeMap<String, String>, table: &str, name: &str, expected_ty: &str) {
    let actual = cols
        .get(name)
        .unwrap_or_else(|| panic!("table `{table}` missing column `{name}`; have: {cols:?}"));
    assert_eq!(
        actual.as_str(),
        expected_ty,
        "table `{table}` column `{name}` has type `{actual}`, expected `{expected_ty}`"
    );
}

#[tokio::test]
async fn migrator_applies_to_fresh_database() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;

    harness_storage::MIGRATOR
        .run(&pool)
        .await
        .expect("migrator should apply cleanly to a fresh database");

    for table in [
        "providers_config",
        "conversations",
        "messages",
        "settings",
        "sandbox_templates",
    ] {
        assert!(
            table_exists(&pool, table).await,
            "expected table `{table}` after migration"
        );
    }
}

#[tokio::test]
async fn providers_config_has_expected_columns() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;
    harness_storage::MIGRATOR.run(&pool).await.expect("migrate");

    let cols = columns(&pool, "providers_config").await;
    assert_column(&cols, "providers_config", "provider_id", "TEXT");
    assert_column(&cols, "providers_config", "config_json", "BLOB");
    assert_column(&cols, "providers_config", "updated_at", "INTEGER");
}

#[tokio::test]
async fn conversations_has_expected_columns() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;
    harness_storage::MIGRATOR.run(&pool).await.expect("migrate");

    let cols = columns(&pool, "conversations").await;
    assert_column(&cols, "conversations", "id", "TEXT");
    assert_column(&cols, "conversations", "title", "TEXT");
    assert_column(&cols, "conversations", "provider_id", "TEXT");
    assert_column(&cols, "conversations", "model", "TEXT");
    assert_column(&cols, "conversations", "sandbox_template_id", "TEXT");
    assert_column(&cols, "conversations", "created_at", "INTEGER");
    assert_column(&cols, "conversations", "updated_at", "INTEGER");
}

#[tokio::test]
async fn messages_has_expected_columns_and_index() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;
    harness_storage::MIGRATOR.run(&pool).await.expect("migrate");

    let cols = columns(&pool, "messages").await;
    assert_column(&cols, "messages", "id", "TEXT");
    assert_column(&cols, "messages", "conversation_id", "TEXT");
    assert_column(&cols, "messages", "role", "TEXT");
    assert_column(&cols, "messages", "content_json", "BLOB");
    assert_column(&cols, "messages", "created_at", "INTEGER");
    assert_column(&cols, "messages", "ordinal", "INTEGER");

    assert!(
        index_exists(&pool, "idx_messages_conv_ord").await,
        "expected idx_messages_conv_ord index after migration"
    );
}

#[tokio::test]
async fn settings_has_expected_columns() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;
    harness_storage::MIGRATOR.run(&pool).await.expect("migrate");

    let cols = columns(&pool, "settings").await;
    assert_column(&cols, "settings", "key", "TEXT");
    assert_column(&cols, "settings", "value_json", "BLOB");
}

#[tokio::test]
async fn sandbox_templates_has_expected_columns() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;
    harness_storage::MIGRATOR.run(&pool).await.expect("migrate");

    let cols = columns(&pool, "sandbox_templates").await;
    assert_column(&cols, "sandbox_templates", "id", "TEXT");
    assert_column(&cols, "sandbox_templates", "name", "TEXT");
    assert_column(&cols, "sandbox_templates", "description", "TEXT");
    assert_column(&cols, "sandbox_templates", "profile", "TEXT");
    assert_column(&cols, "sandbox_templates", "is_builtin", "INTEGER");
    assert_column(&cols, "sandbox_templates", "created_at", "INTEGER");
    assert_column(&cols, "sandbox_templates", "updated_at", "INTEGER");
}

#[tokio::test]
async fn migrator_is_idempotent() {
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;

    harness_storage::MIGRATOR
        .run(&pool)
        .await
        .expect("first migrate");
    harness_storage::MIGRATOR
        .run(&pool)
        .await
        .expect("re-applying the migrator must be a no-op");

    // Tables still present after the second run.
    for table in [
        "providers_config",
        "conversations",
        "messages",
        "settings",
        "sandbox_templates",
    ] {
        assert!(table_exists(&pool, table).await, "table `{table}` lost");
    }
}

#[tokio::test]
async fn foreign_keys_enforced_when_pragma_enabled() {
    // Per spec: messages.conversation_id ON DELETE CASCADE,
    // conversations.sandbox_template_id ON DELETE SET NULL.
    // We don't test cascade behavior here (that belongs to repo CRUD tests in T1.B);
    // we just assert the FK clauses are declared so PRAGMA foreign_key_list reports them.
    let tmp = NamedTempFile::new().expect("tempfile");
    let pool = connect(tmp.path()).await;
    harness_storage::MIGRATOR.run(&pool).await.expect("migrate");

    let messages_fks = sqlx::query("PRAGMA foreign_key_list(messages)")
        .fetch_all(&pool)
        .await
        .expect("fk list messages");
    assert!(
        messages_fks.iter().any(|r| {
            let table: String = r.try_get("table").unwrap_or_default();
            let on_delete: String = r.try_get("on_delete").unwrap_or_default();
            table == "conversations" && on_delete.eq_ignore_ascii_case("CASCADE")
        }),
        "messages must declare FK to conversations ON DELETE CASCADE"
    );

    let conv_fks = sqlx::query("PRAGMA foreign_key_list(conversations)")
        .fetch_all(&pool)
        .await
        .expect("fk list conversations");
    assert!(
        conv_fks.iter().any(|r| {
            let table: String = r.try_get("table").unwrap_or_default();
            let on_delete: String = r.try_get("on_delete").unwrap_or_default();
            table == "sandbox_templates" && on_delete.eq_ignore_ascii_case("SET NULL")
        }),
        "conversations must declare FK to sandbox_templates ON DELETE SET NULL"
    );
}
