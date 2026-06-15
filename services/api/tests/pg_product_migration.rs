//! Behavior: the cloud Postgres product-migration runner (issue #64).
//!
//! Proves, through the public API, that a product's Postgres migrations plug into
//! the cloud spine the same way `LocalModule` migrations plug into the desktop:
//! additively behind the baseline, idempotently, namespace-prefixed, and with two
//! products provably non-colliding. This mirrors the local-store migration tests
//! (`crates/local-store/tests`), but on the cloud side and against the
//! driver-agnostic [`api::pg_migration::InMemoryPgExecutor`] (there is no live
//! Postgres in-tree yet; the sqlx-backed executor lands with the durable store).
//!
//! The product modules here are tiny in-test doubles (NOT a real product): just
//! enough to exercise the runner's four guarantees through `apply_module`.

use axum::routing::get;
use axum::Router;

use api::pg_migration::{InMemoryPgExecutor, PgMigration, BASELINE_MIGRATIONS};
use api::product_module::{apply_module, BackendModule};
use shared::ProductModuleMeta;

/// A product ("orinsync") that contributes one namespaced cloud table — the
/// shape OrinSync P2 needs for its corpus. Its version and table carry the
/// `orinsync_` prefix the convention requires.
struct OrinSyncModule;

const ORINSYNC_MIGRATIONS: &[PgMigration] = &[PgMigration {
    version: "orinsync_0001_init",
    sql: "CREATE TABLE orinsync_corpus (id TEXT PRIMARY KEY, body TEXT NOT NULL);",
}];

impl BackendModule for OrinSyncModule {
    fn meta(&self) -> ProductModuleMeta {
        ProductModuleMeta::new("OrinSync", "orinsync").expect("valid namespace")
    }
    fn router(&self) -> Router {
        Router::new().route("/corpus", get(|| async { "orinsync" }))
    }
    fn migrations(&self) -> &'static [PgMigration] {
        ORINSYNC_MIGRATIONS
    }
}

/// A second product ("vault") to prove two namespaces never collide in the shared
/// `schema_migrations` ledger.
struct VaultModule;

const VAULT_MIGRATIONS: &[PgMigration] = &[PgMigration {
    version: "vault_0001_init",
    sql: "CREATE TABLE vault_records (id TEXT PRIMARY KEY, body TEXT NOT NULL);",
}];

impl BackendModule for VaultModule {
    fn meta(&self) -> ProductModuleMeta {
        ProductModuleMeta::new("Vault", "vault").expect("valid namespace")
    }
    fn router(&self) -> Router {
        Router::new().route("/records", get(|| async { "vault" }))
    }
    fn migrations(&self) -> &'static [PgMigration] {
        VAULT_MIGRATIONS
    }
}

// --- ISC-25: a product's migrations apply additively behind the baseline ---
#[test]
fn product_migrations_apply_after_the_baseline() {
    let mut exec = InMemoryPgExecutor::new();
    let applied = apply_module(&mut exec, &OrinSyncModule).unwrap();
    assert_eq!(
        applied,
        BASELINE_MIGRATIONS.len() + ORINSYNC_MIGRATIONS.len(),
        "baseline migrations plus the product's"
    );
    // The baseline's ledger version is present, and so is the product's.
    let versions: Vec<&str> = exec.ran().iter().map(|(v, _)| v.as_str()).collect();
    assert!(versions.contains(&"0001_init"), "baseline applied");
    assert!(versions.contains(&"orinsync_0001_init"), "product applied");
}

// --- ISC-26: applying a product's migrations twice is idempotent ---
#[test]
fn applying_a_product_twice_is_idempotent() {
    let mut exec = InMemoryPgExecutor::new();
    apply_module(&mut exec, &OrinSyncModule).unwrap();
    let second = apply_module(&mut exec, &OrinSyncModule).unwrap();
    assert_eq!(
        second, 0,
        "the schema_migrations ledger prevents re-application"
    );
}

// --- ISC-27: the product's table and version carry the namespace prefix ---
#[test]
fn product_table_uses_the_namespace_prefix() {
    let prefix = OrinSyncModule.meta().table_prefix();
    assert!(
        ORINSYNC_MIGRATIONS[0].version.starts_with(&prefix),
        "migration version is namespace-prefixed"
    );
    assert!(
        ORINSYNC_MIGRATIONS[0]
            .sql
            .contains(&format!("{prefix}corpus")),
        "the table name is namespace-prefixed"
    );
}

// --- ISC-28: two products never collide; one runner never touches the other ---
#[test]
fn two_products_never_collide() {
    let mut exec = InMemoryPgExecutor::new();

    // Apply OrinSync first. Its run must emit only the baseline + orinsync
    // versions — never a vault version.
    apply_module(&mut exec, &OrinSyncModule).unwrap();
    let after_orinsync: Vec<String> = exec.applied_versions().to_vec();
    assert!(
        after_orinsync.iter().all(|v| !v.starts_with("vault_")),
        "OrinSync's run never emits another product's versions"
    );

    // Apply Vault against the SAME executor (shared ledger). Both products' tables
    // now exist; the baseline was not re-applied.
    let vault_applied = apply_module(&mut exec, &VaultModule).unwrap();
    assert_eq!(
        vault_applied,
        VAULT_MIGRATIONS.len(),
        "only Vault's own migration applies; the shared baseline is already recorded"
    );

    let all: Vec<&str> = exec.applied_versions().iter().map(|s| s.as_str()).collect();
    assert!(all.contains(&"orinsync_0001_init"));
    assert!(all.contains(&"vault_0001_init"));

    // Re-applying OrinSync now is still a no-op: Vault's presence did not disturb
    // OrinSync's recorded state, and OrinSync does not re-touch Vault.
    let reapply = apply_module(&mut exec, &OrinSyncModule).unwrap();
    assert_eq!(
        reapply, 0,
        "each product's ledger state is independent and stable"
    );
}

/// A product that tries to claim another product's already-namespaced version
/// (`vault_0001_init`) instead of its own `ghost_` prefix. The shared ledger is
/// keyed on the raw string, so without enforcement this would silently suppress
/// `ghost`'s own table after Vault ran. The prefix guard rejects it up front.
struct GhostModule;

const GHOST_MIGRATIONS: &[PgMigration] = &[PgMigration {
    version: "vault_0001_init",
    sql: "CREATE TABLE ghost_records (id TEXT PRIMARY KEY);",
}];

impl BackendModule for GhostModule {
    fn meta(&self) -> ProductModuleMeta {
        ProductModuleMeta::new("Ghost", "ghost").expect("valid namespace")
    }
    fn router(&self) -> Router {
        Router::new()
    }
    fn migrations(&self) -> &'static [PgMigration] {
        GHOST_MIGRATIONS
    }
}

// --- ISC-28 (negative): a product can never collide by borrowing another's
// version — the namespace prefix is enforced, not just hoped for ---
#[test]
fn a_product_cannot_claim_another_products_version() {
    let mut exec = InMemoryPgExecutor::new();
    // Vault legitimately owns `vault_0001_init`.
    apply_module(&mut exec, &VaultModule).unwrap();
    // Ghost tries to reuse it. Rejected because it does not start with `ghost_`.
    let result = apply_module(&mut exec, &GhostModule);
    assert!(
        result.is_err(),
        "a mis-prefixed product version must be rejected, not silently skipped"
    );
}
