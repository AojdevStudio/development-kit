//! Persistence test: state survives a simulated app restart.
//!
//! This is the acceptance-criteria test for issue #33 ("State survives an app
//! restart (tested)"). It:
//!   1. Opens a real SQLite file in a temp directory.
//!   2. Applies all migrations.
//!   3. Writes a draft through the repository.
//!   4. Drops the connection (simulates an app shutdown).
//!   5. Reopens the same file (simulates an app restart).
//!   6. Reads back the draft and asserts it is identical to what was written.

use local_store::db::LocalDb;
use local_store::draft::DraftRepository;
use tempfile::TempDir;

/// Helper: create a temp dir whose lifetime binds to the test scope.
fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

#[test]
fn draft_survives_connection_close_and_reopen() {
    let dir = temp_dir();
    let db_path = dir.path().join("local.db");

    // -- Write phase (app session 1) -------------------------------------------
    let id = {
        let db = LocalDb::open(&db_path).expect("open session 1");
        let repo = DraftRepository::new(db.conn());
        repo.save("Restart test", "body that must survive")
            .expect("save draft")
        // `db` drops here → connection closed
    };

    // -- Read phase (app session 2) --------------------------------------------
    let db = LocalDb::open(&db_path).expect("open session 2");
    let repo = DraftRepository::new(db.conn());
    let draft = repo.find(id).expect("draft must exist after restart");

    assert_eq!(draft.id, id);
    assert_eq!(draft.title, "Restart test");
    assert_eq!(draft.body, "body that must survive");
}

#[test]
fn migrations_applied_exactly_once_across_multiple_opens() {
    let dir = temp_dir();
    let db_path = dir.path().join("local.db");

    // First open — runs all migrations.
    {
        let _db = LocalDb::open(&db_path).expect("first open");
    }

    // Second open — migrations must be idempotent; no error, same row count.
    let db = LocalDb::open(&db_path).expect("second open");
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("schema_migrations query");

    // We have exactly 2 migrations (0001_init, 0002_local_product_state).
    assert_eq!(
        count, 2,
        "exactly 2 migration rows; second open must not re-insert them"
    );
}

#[test]
fn local_drafts_table_exists_after_migration() {
    let db = LocalDb::open_in_memory().expect("in-memory DB");
    // Verify the 0002 migration ran and the table is present.
    let result: rusqlite::Result<i64> =
        db.conn()
            .query_row("SELECT COUNT(*) FROM local_drafts", [], |row| row.get(0));
    assert!(
        result.is_ok(),
        "local_drafts table must exist after migration"
    );
}

#[test]
fn sync_queue_table_exists_after_migration() {
    let db = LocalDb::open_in_memory().expect("in-memory DB");
    let result: rusqlite::Result<i64> =
        db.conn()
            .query_row("SELECT COUNT(*) FROM sync_queue", [], |row| row.get(0));
    assert!(
        result.is_ok(),
        "sync_queue table must exist after migration"
    );
}

#[test]
fn no_authoritative_billing_tables_in_local_store() {
    // ADR-0001: local SQLite must never store authoritative billing state.
    // This test verifies that known authoritative table names do not exist.
    let db = LocalDb::open_in_memory().expect("in-memory DB");
    let forbidden = [
        "subscriptions",
        "stripe_events",
        "entitlements",
        "license_tokens",
    ];
    for table in &forbidden {
        let exists: bool = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(false);
        assert!(
            !exists,
            "authoritative billing table '{table}' must not exist in the local store (ADR-0001)"
        );
    }
}
