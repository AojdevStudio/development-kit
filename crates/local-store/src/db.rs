//! LocalDb — the single entry point for opening the local SQLite store.
//!
//! Call `LocalDb::open(path)` to get a fully-migrated connection. In tests,
//! use `LocalDb::open_in_memory()` for a throwaway DB.

use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;
use crate::migration::{run_migrations, MIGRATIONS};

/// A migrated local SQLite connection.
///
/// This is the public entry point: open it once at app startup (or at test
/// setup), then build repositories from it via `LocalDb::conn()`.
pub struct LocalDb {
    conn: Connection,
}

impl LocalDb {
    /// Open the SQLite file at `path`, creating it if it does not exist, and
    /// apply all pending migrations. Returns an error if any migration fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::configure_and_migrate(conn)
    }

    /// Open an in-memory database and apply all migrations. Useful in tests
    /// where a temp file is too heavy.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_and_migrate(conn)
    }

    fn configure_and_migrate(conn: Connection) -> Result<Self> {
        // WAL mode for better concurrent read performance; safe for single-writer use.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        run_migrations(&conn, MIGRATIONS)?;
        Ok(LocalDb { conn })
    }

    /// Borrow the underlying connection so repositories can be constructed.
    ///
    /// Repositories take `&Connection` and are cheap to construct — just pass
    /// `db.conn()` at the call site.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_returns_migrated_db() {
        let db = LocalDb::open_in_memory().expect("should open");
        // Confirm the tracking table exists and has rows.
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("schema_migrations must exist after migration");
        assert!(count > 0, "at least one migration must be recorded");
    }
}
