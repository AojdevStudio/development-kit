//! The desktop half of the product-module seam (issue #36).
//!
//! A product module contributes its local SQLite schema by implementing
//! [`LocalModule`]: it returns its [`ProductModuleMeta`] (identity + namespace)
//! and the [`Migration`]s for its local tables. [`apply_module`] runs the spine's
//! baseline migrations and then the product's, so a product's local schema plugs
//! in **additively** on top of the platform schema without editing the baseline
//! [`MIGRATIONS`](crate::migration::MIGRATIONS) list.
//!
//! This is the complement of `services/api`'s `BackendModule`: that side names
//! `axum::Router`, this side names [`Migration`] (ADR-0002 keeps the two
//! authority sides' dependencies apart). Both sides share the namespace through
//! [`ProductModuleMeta`] in the dependency-thin `shared` crate, so a product's
//! routes (`/<namespace>/…`), feature keys (`<namespace>.…`), and tables
//! (`<namespace>_…`) all derive from one value.

use rusqlite::Connection;
use shared::ProductModuleMeta;

use crate::error::Result;
use crate::migration::{run_migrations, Migration, MIGRATIONS};

/// A product module's **local** contribution: its identity and its SQLite
/// migrations.
///
/// By convention (see `docs/PRODUCT-MODULE-SEAM.md`) a product names its tables
/// `<namespace>_<entity>` and its migration versions `<namespace>_NNNN_…`, so
/// two products never collide in the shared `schema_migrations` ledger.
pub trait LocalModule {
    /// The module's stable identity and namespace.
    fn meta(&self) -> ProductModuleMeta;

    /// The product's local SQLite migrations, applied after the baseline schema.
    fn migrations(&self) -> &'static [Migration];
}

/// Apply the baseline schema and then `module`'s migrations to `conn`, in order.
///
/// Returns the total number of migrations applied. Idempotent: running it twice
/// applies the product's migrations only once, because the runner records each
/// version in `schema_migrations` and skips already-applied versions — exactly
/// the same contract the baseline migrations use, so a product's schema is as
/// safe to re-apply as the spine's.
pub fn apply_module<M: LocalModule + ?Sized>(conn: &Connection, module: &M) -> Result<usize> {
    let baseline = run_migrations(conn, MIGRATIONS)?;
    let product = run_migrations(conn, module.migrations())?;
    Ok(baseline + product)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny in-test product ("vault") with one local table, named with the
    /// `<namespace>_` prefix the convention requires.
    struct VaultLocal;

    const VAULT_MIGRATIONS: &[Migration] = &[Migration {
        version: "vault_0001_init",
        sql: "CREATE TABLE vault_records (id TEXT PRIMARY KEY, body TEXT NOT NULL);",
    }];

    impl LocalModule for VaultLocal {
        fn meta(&self) -> ProductModuleMeta {
            ProductModuleMeta::new("Vault", "vault").expect("valid namespace")
        }
        fn migrations(&self) -> &'static [Migration] {
            VAULT_MIGRATIONS
        }
    }

    fn memory() -> Connection {
        Connection::open_in_memory().expect("in-memory DB")
    }

    // --- ISC-22: product migrations apply on top of the baseline ---
    #[test]
    fn applying_a_module_runs_baseline_then_product_migrations() {
        let conn = memory();
        let applied = apply_module(&conn, &VaultLocal).unwrap();
        // baseline migrations + the one vault migration
        assert_eq!(applied, MIGRATIONS.len() + VAULT_MIGRATIONS.len());
    }

    // --- ISC-23: applying twice is idempotent ---
    #[test]
    fn applying_a_module_twice_is_idempotent() {
        let conn = memory();
        apply_module(&conn, &VaultLocal).unwrap();
        let second = apply_module(&conn, &VaultLocal).unwrap();
        assert_eq!(second, 0, "second run applies nothing");
    }

    // --- ISC-24: the product table is queryable after apply ---
    #[test]
    fn product_table_is_queryable_after_apply() {
        let conn = memory();
        apply_module(&conn, &VaultLocal).unwrap();
        conn.execute(
            "INSERT INTO vault_records (id, body) VALUES (?1, ?2)",
            ["rec_1", "hello"],
        )
        .expect("vault_records table exists and is writable");
        let body: String = conn
            .query_row(
                "SELECT body FROM vault_records WHERE id = ?1",
                ["rec_1"],
                |r| r.get(0),
            )
            .expect("row reads back");
        assert_eq!(body, "hello");
    }

    #[test]
    fn product_table_uses_the_namespace_prefix() {
        // The convention is mechanical: the table name starts with the module's
        // table prefix, so a reviewer can see which product owns it.
        let prefix = VaultLocal.meta().table_prefix();
        assert!(VAULT_MIGRATIONS[0]
            .sql
            .contains(&format!("{prefix}records")));
    }
}
