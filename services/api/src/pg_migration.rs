//! Migration runner for the cloud Postgres authority store.
//!
//! The cloud-side mirror of `crates/local-store`'s SQLite migration runner
//! (`migration.rs` + `product_module.rs`). Conventions (see `migrations/README.md`
//! and `docs/PRODUCT-MODULE-SEAM.md`):
//! - Migration files are ordered, append-only, `NNNN_description.sql`; a product's
//!   migrations are namespaced `<namespace>_NNNN_<desc>` so two products never
//!   collide in the shared `schema_migrations` ledger.
//! - The `schema_migrations` table (bootstrapped by `postgres/0001_init`) records
//!   applied versions; already-applied versions are skipped on re-run.
//!
//! There is no live Postgres driver in this workspace yet: the backend store is
//! in-memory and the durable Postgres-backed store is a later issue (see
//! `store.rs`). So this runner is **driver-agnostic**. It takes the migrations as
//! a `&[PgMigration]` slice and drives a [`PgExecutor`] trait, exactly as the
//! desktop runner takes a `&[Migration]` slice and a rusqlite `Connection`. The
//! concrete sqlx-backed [`PgExecutor`] ships with the durable store; this module
//! is unit-tested against [`InMemoryPgExecutor`], mirroring the local-store
//! migration tests, and that in-memory executor is the no-database backing the
//! walking skeleton uses (the same mock-now / real-later shape as
//! `InMemoryPrincipalStore` and `MockWebhookVerifier`).

use thiserror::Error;

/// A single Postgres migration: a stable version string and the SQL to run.
///
/// Mirrors `local_store::migration::Migration`. The SQL may contain multiple
/// statements; the [`PgExecutor`] decides how to run them.
#[derive(Debug, Clone)]
pub struct PgMigration {
    /// Stable identifier for this migration, e.g. `"0001_init"` or
    /// `"orinsync_0001_init"`.
    pub version: &'static str,
    /// The SQL to execute for this migration.
    pub sql: &'static str,
}

/// Typed errors for the Postgres migration runner. Mirrors the
/// `Migration(String)` variant of `local_store::error::LocalStoreError`.
#[derive(Debug, Error)]
pub enum PgMigrationError {
    /// A migration could not be applied (bad SQL, executor/connection failure).
    #[error("migration error: {0}")]
    Migration(String),
}

/// Convenience result type for the runner.
pub type Result<T> = std::result::Result<T, PgMigrationError>;

/// The driver-agnostic seam between the runner and a Postgres connection.
///
/// A real implementation wraps a Postgres connection or transaction; the runner
/// never names a concrete driver, so this slice adds no Postgres client crate to
/// the workspace. The contract mirrors what the desktop runner does inline
/// against a rusqlite `Connection`:
///
/// - [`ensure_ledger`](PgExecutor::ensure_ledger): create the `schema_migrations`
///   table if it does not exist (defensive: `postgres/0001_init` also creates it).
/// - [`is_applied`](PgExecutor::is_applied): has this version already been recorded?
/// - [`apply`](PgExecutor::apply): run the migration SQL and record its version in
///   the ledger **atomically** (one transaction), so a failed SQL statement never
///   leaves a recorded version behind.
pub trait PgExecutor {
    /// Create the `schema_migrations` ledger table if absent.
    fn ensure_ledger(&mut self) -> Result<()>;

    /// Return whether `version` is already recorded in the ledger.
    fn is_applied(&self, version: &str) -> Result<bool>;

    /// Run `sql` and record `version` in the ledger atomically. An error means
    /// nothing was committed: the version must NOT be recorded.
    ///
    /// **Contract for the real executor:** the DDL and the ledger insert MUST run
    /// inside one transaction (`BEGIN … COMMIT`), so a crash or a failing
    /// statement leaves the database with neither the schema change nor the
    /// version row. [`InMemoryPgExecutor`] cannot prove this (it runs no real
    /// SQL and cannot roll back); the sqlx-backed executor that lands with the
    /// durable store owes contract tests for "DDL succeeds but ledger insert
    /// fails" and for concurrent runners (advisory lock). These are tracked as
    /// deferred follow-up work for #64 (see that PR's description).
    fn apply(&mut self, version: &str, sql: &str) -> Result<()>;
}

/// Apply all pending `migrations` against `executor`, in slice order.
///
/// Ensures the ledger exists, skips already-applied versions, applies the rest,
/// and returns the number applied. Mirrors `local_store::migration::run_migrations`:
/// the runner only ever applies versions present in the passed slice, so it can
/// never touch the baseline's or another product's tables.
///
/// Rejects a slice that repeats a version (returns [`PgMigrationError::Migration`]
/// before applying anything). Without this guard a duplicate would be silently
/// skipped as "already applied" after its first sibling ran, so its SQL would
/// never execute while the run still reported success: a silent data-loss footgun
/// the shared `schema_migrations` ledger makes possible.
pub fn run_migrations<E: PgExecutor + ?Sized>(
    executor: &mut E,
    migrations: &[PgMigration],
) -> Result<usize> {
    reject_duplicate_versions(migrations)?;

    // Bootstrapped by postgres/0001_init, but created defensively here so the
    // runner works against a brand-new database that has not run it yet.
    executor.ensure_ledger()?;

    let mut applied = 0usize;
    for m in migrations {
        if executor.is_applied(m.version)? {
            continue;
        }
        executor.apply(m.version, m.sql)?;
        applied += 1;
    }
    Ok(applied)
}

/// Fail if any version appears more than once in `migrations`. A repeated version
/// in one slice would self-suppress: the second occurrence reads as already
/// applied and is skipped, so its SQL never runs even though the run succeeds.
pub(crate) fn reject_duplicate_versions(migrations: &[PgMigration]) -> Result<()> {
    let mut seen = std::collections::HashSet::with_capacity(migrations.len());
    for m in migrations {
        if !seen.insert(m.version) {
            return Err(PgMigrationError::Migration(format!(
                "duplicate migration version '{}' in the slice",
                m.version
            )));
        }
    }
    Ok(())
}

/// The embedded baseline Postgres migration (the cloud bootstrap), sourced from
/// `migrations/postgres/0001_init.sql`. Mirrors the desktop `MIGRATIONS` const,
/// which embeds the SQLite baseline the same way. The durable SaaS authority
/// schema lands in its own issues; this is only the `schema_migrations` ledger.
pub const BASELINE_MIGRATIONS: &[PgMigration] = &[PgMigration {
    version: "0001_init",
    sql: include_str!("../../../migrations/postgres/0001_init.sql"),
}];

/// The no-database [`PgExecutor`] for the walking skeleton and tests.
///
/// It keeps the `schema_migrations` ledger in memory and records the SQL it was
/// asked to run, so the runner's logic (skip-applied, apply-pending, record
/// atomically) is exercised end-to-end with no live Postgres. This is the same
/// mock-now / real-later pattern the backend already uses for
/// `InMemoryPrincipalStore`, `MockBillingProvider`, and `MockWebhookVerifier`;
/// the sqlx-backed executor that runs real DDL drops in behind the same trait
/// when the durable Postgres store lands.
#[derive(Debug, Default, Clone)]
pub struct InMemoryPgExecutor {
    ledger_ready: bool,
    applied: Vec<String>,
    ran_sql: Vec<(String, String)>,
}

impl InMemoryPgExecutor {
    /// A fresh executor with an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether [`ensure_ledger`](PgExecutor::ensure_ledger) has run.
    pub fn ledger_ready(&self) -> bool {
        self.ledger_ready
    }

    /// The recorded migration versions, in application order.
    pub fn applied_versions(&self) -> &[String] {
        &self.applied
    }

    /// The `(version, sql)` pairs the runner asked this executor to apply, in
    /// order. Lets a test assert exactly which versions were emitted (and that
    /// none belong to the baseline or another product).
    pub fn ran(&self) -> &[(String, String)] {
        &self.ran_sql
    }
}

impl PgExecutor for InMemoryPgExecutor {
    fn ensure_ledger(&mut self) -> Result<()> {
        self.ledger_ready = true;
        Ok(())
    }

    fn is_applied(&self, version: &str) -> Result<bool> {
        Ok(self.applied.iter().any(|v| v == version))
    }

    fn apply(&mut self, version: &str, sql: &str) -> Result<()> {
        // Record atomically: in this in-memory backing both pushes happen
        // together, so there is no partial state. A real executor wraps the DDL
        // and the ledger insert in one transaction for the same guarantee.
        self.ran_sql.push((version.to_string(), sql.to_string()));
        self.applied.push(version.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An executor that fails on [`apply`](PgExecutor::apply), standing in for a
    /// real connection rejecting bad SQL. It records the ledger state so a test
    /// can prove a failed apply leaves NO recorded version (atomicity).
    #[derive(Default)]
    struct FailingExecutor {
        applied: Vec<String>,
    }

    impl PgExecutor for FailingExecutor {
        fn ensure_ledger(&mut self) -> Result<()> {
            Ok(())
        }
        fn is_applied(&self, version: &str) -> Result<bool> {
            Ok(self.applied.iter().any(|v| v == version))
        }
        fn apply(&mut self, version: &str, _sql: &str) -> Result<()> {
            // Fail before recording: a real connection that errors on the DDL
            // rolls back, so the version is never written.
            Err(PgMigrationError::Migration(format!(
                "applying '{version}': simulated bad SQL"
            )))
        }
    }

    fn baseline_versions() -> Vec<&'static str> {
        BASELINE_MIGRATIONS.iter().map(|m| m.version).collect()
    }

    // --- ISC-9: a fresh executor applies every baseline migration ---
    #[test]
    fn fresh_executor_applies_all_baseline_migrations() {
        let mut exec = InMemoryPgExecutor::new();
        let applied = run_migrations(&mut exec, BASELINE_MIGRATIONS).unwrap();
        assert_eq!(applied, BASELINE_MIGRATIONS.len());
        assert!(exec.ledger_ready(), "the ledger must be ensured first");
    }

    // --- ISC-10: running twice applies nothing the second time ---
    #[test]
    fn running_migrations_twice_is_idempotent() {
        let mut exec = InMemoryPgExecutor::new();
        run_migrations(&mut exec, BASELINE_MIGRATIONS).unwrap();
        let second = run_migrations(&mut exec, BASELINE_MIGRATIONS).unwrap();
        assert_eq!(second, 0, "second run applies nothing");
    }

    // --- ISC-11: the ledger records applied versions in order ---
    #[test]
    fn ledger_records_applied_versions_in_order() {
        let mut exec = InMemoryPgExecutor::new();
        run_migrations(&mut exec, BASELINE_MIGRATIONS).unwrap();
        assert_eq!(exec.applied_versions(), baseline_versions().as_slice());
    }

    // --- ISC-14: a pre-recorded version is skipped ---
    #[test]
    fn already_recorded_version_is_skipped() {
        let mut exec = InMemoryPgExecutor::new();
        // First run records the baseline.
        run_migrations(&mut exec, BASELINE_MIGRATIONS).unwrap();
        let before = exec.applied_versions().len();
        // A slice whose only entry is an already-recorded version applies 0.
        let again = run_migrations(&mut exec, BASELINE_MIGRATIONS).unwrap();
        assert_eq!(again, 0);
        assert_eq!(
            exec.applied_versions().len(),
            before,
            "no new rows recorded"
        );
    }

    // --- ISC-12: an executor failure surfaces as PgMigrationError::Migration ---
    #[test]
    fn executor_failure_surfaces_as_migration_error() {
        let mut exec = FailingExecutor::default();
        let bad = [PgMigration {
            version: "9999_bad",
            sql: "THIS IS NOT SQL;",
        }];
        match run_migrations(&mut exec, &bad) {
            Err(PgMigrationError::Migration(_)) => {}
            other => panic!("expected Migration error, got {other:?}"),
        }
    }

    // --- ISC-13: a failed apply records no version (atomic) ---
    #[test]
    fn failed_apply_records_no_version() {
        let mut exec = FailingExecutor::default();
        let bad = [PgMigration {
            version: "9999_bad",
            sql: "boom",
        }];
        let _ = run_migrations(&mut exec, &bad);
        assert!(
            !exec.is_applied("9999_bad").unwrap(),
            "a failed apply must not leave a recorded version"
        );
    }

    // --- ISC-37: a slice that repeats a version is rejected before applying ---
    // (Forge HIGH/MED: a duplicate would self-suppress via the shared ledger and
    // its SQL would never run while the run still reported success.)
    #[test]
    fn duplicate_versions_in_a_slice_are_rejected() {
        let mut exec = InMemoryPgExecutor::new();
        let dupes = [
            PgMigration {
                version: "p_0001",
                sql: "CREATE TABLE a (id TEXT);",
            },
            PgMigration {
                version: "p_0001",
                sql: "CREATE TABLE b (id TEXT);",
            },
        ];
        match run_migrations(&mut exec, &dupes) {
            Err(PgMigrationError::Migration(msg)) => assert!(msg.contains("duplicate")),
            other => panic!("expected duplicate-version error, got {other:?}"),
        }
        // Nothing was applied: the guard runs before any executor call.
        assert!(
            exec.applied_versions().is_empty(),
            "a rejected slice applies nothing"
        );
    }
}
