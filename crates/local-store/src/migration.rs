//! Migration runner for the local SQLite store.
//!
//! Conventions (from migrations/README.md):
//! - Files are ordered, append-only, named `NNNN_description.sql`.
//! - Migration versions are the filename stems (e.g. `0001_init`).
//! - The `schema_migrations` table (created by 0001_init) tracks what has run.
//! - Migrations run in version order; already-applied versions are skipped.
//!
//! The runner takes the migration SQL as a `&[Migration]` slice so it is
//! testable without the filesystem.

use rusqlite::Connection;

use crate::error::{LocalStoreError, Result};

/// A single migration: a stable version string and the SQL to run.
#[derive(Debug, Clone)]
pub struct Migration {
    /// Stable identifier for this migration, e.g. `"0001_init"`.
    pub version: &'static str,
    /// The SQL to execute. May contain multiple statements separated by `;`.
    pub sql: &'static str,
}

/// Apply all pending migrations to `conn` in version order.
///
/// Each migration is wrapped in a transaction: the version is recorded in
/// `schema_migrations` only after the SQL succeeds. Already-applied versions
/// are silently skipped.
pub fn run_migrations(conn: &Connection, migrations: &[Migration]) -> Result<usize> {
    // Ensure the tracking table exists first (bootstrapped by 0001_init, but we
    // create it defensively here so the runner works even against a brand-new DB
    // that hasn't run migration 0001 yet).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     TEXT PRIMARY KEY,
            applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(|e| LocalStoreError::Migration(format!("bootstrap table: {e}")))?;

    let mut applied = 0usize;

    for m in migrations {
        // Check if this version is already recorded.
        let already_applied: bool = conn
            .prepare("SELECT 1 FROM schema_migrations WHERE version = ?1")?
            .exists([m.version])?;

        if already_applied {
            continue;
        }

        // Run the migration SQL and record the version atomically.
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version) VALUES ('{}');\nCOMMIT;",
            m.sql, m.version
        ))
        .map_err(|e| {
            LocalStoreError::Migration(format!("applying migration '{}': {e}", m.version))
        })?;

        applied += 1;
    }

    Ok(applied)
}

/// The embedded SQLite migrations for the local store.
///
/// `0001_init` is sourced from `migrations/sqlite/0001_init.sql` (the walking
/// skeleton). `0002_local_product_state` adds the local product tables this
/// issue requires.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: "0001_init",
        sql: include_str!("../../../migrations/sqlite/0001_init.sql"),
    },
    Migration {
        version: "0002_local_product_state",
        sql: include_str!("../../../migrations/sqlite/0002_local_product_state.sql"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn open_memory() -> Connection {
        Connection::open_in_memory().expect("in-memory DB")
    }

    #[test]
    fn fresh_db_applies_all_migrations() {
        let conn = open_memory();
        let applied = run_migrations(&conn, MIGRATIONS).unwrap();
        assert_eq!(
            applied,
            MIGRATIONS.len(),
            "all migrations should run on a fresh DB"
        );
    }

    #[test]
    fn running_migrations_twice_is_idempotent() {
        let conn = open_memory();
        run_migrations(&conn, MIGRATIONS).unwrap();
        let second_run = run_migrations(&conn, MIGRATIONS).unwrap();
        assert_eq!(second_run, 0, "second run should apply 0 migrations");
    }

    #[test]
    fn schema_migrations_table_records_applied_versions() {
        let conn = open_memory();
        run_migrations(&conn, MIGRATIONS).unwrap();

        let versions: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT version FROM schema_migrations ORDER BY version")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        let expected: Vec<&str> = MIGRATIONS.iter().map(|m| m.version).collect();
        assert_eq!(versions, expected);
    }

    #[test]
    fn bad_sql_returns_migration_error() {
        let conn = open_memory();
        let bad = [Migration {
            version: "9999_bad",
            sql: "THIS IS NOT SQL;",
        }];
        match run_migrations(&conn, &bad) {
            Err(LocalStoreError::Migration(_)) => {}
            other => panic!("expected Migration error, got {other:?}"),
        }
    }
}
