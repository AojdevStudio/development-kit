//! Typed error taxonomy for the local-store crate.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LocalStoreError {
    /// A rusqlite operation failed.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// A migration could not be applied (e.g. bad SQL, version collision).
    #[error("migration error: {0}")]
    Migration(String),

    /// A record that was expected to exist was not found.
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, LocalStoreError>;
