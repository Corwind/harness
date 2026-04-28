//! Map `sqlx::Error` (and friends) to `harness_core::RepoError`.

use harness_core::error::RepoError;

pub(crate) fn sqlx_to_repo(err: sqlx::Error) -> RepoError {
    match &err {
        sqlx::Error::RowNotFound => RepoError::NotFound,
        sqlx::Error::Database(db_err) => {
            // SQLite returns the extended result code as a stringified
            // integer. CONSTRAINT_* all have primary code 19, i.e.
            // `n % 256 == 19` (e.g. 19, 275 ROWID, 787 FOREIGNKEY,
            // 1555 PRIMARYKEY, 2067 UNIQUE).
            let is_constraint = db_err
                .code()
                .as_deref()
                .and_then(|c| c.parse::<u32>().ok())
                .map(|n| n % 256 == 19)
                .unwrap_or(false);
            if is_constraint {
                RepoError::Conflict(db_err.message().to_string())
            } else {
                RepoError::Storage(err.to_string())
            }
        }
        _ => RepoError::Storage(err.to_string()),
    }
}

pub(crate) fn serde_to_repo(err: serde_json::Error) -> RepoError {
    RepoError::Serde(err.to_string())
}
