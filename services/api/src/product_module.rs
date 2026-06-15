//! The backend half of the product-module seam (issue #36).
//!
//! A product module plugs its cloud-side routes into the spine by implementing
//! [`BackendModule`]: it returns its [`ProductModuleMeta`] (identity + namespace)
//! and an `axum::Router` of its product routes. [`mount`] nests that router under
//! the module's namespace prefix (`/<namespace>`), so composition is **additive**
//! — every baseline route stays reachable and two products with distinct
//! namespaces never collide.
//!
//! This is the seam `services/api/src/lib.rs` uses to keep product composition a
//! one-line additive `.merge(mount(&module))` per product, touching no existing
//! route. The trait names `axum::Router`, so it lives here in the backend crate
//! (ADR-0002: the desktop authority side never pulls axum); the desktop's local
//! migrations are contributed through the complementary `LocalModule` trait in
//! `crates/local-store`. What both sides share — the namespace — is
//! [`ProductModuleMeta`] in the dependency-thin `shared` crate.

use axum::Router;
use shared::ProductModuleMeta;

use crate::pg_migration::{
    run_migrations, PgExecutor, PgMigration, PgMigrationError, BASELINE_MIGRATIONS,
};

/// A product module's **backend** contribution: its identity, its cloud routes,
/// and (optionally) its cloud Postgres migrations.
///
/// Implementors return a `Router` of *un-prefixed* product routes (e.g.
/// `/records`); [`mount`] applies the `/<namespace>` prefix from the metadata, so
/// a route is never hardcoded with its namespace and the prefix is declared in
/// exactly one place. The router is returned as a `Router` with its own state
/// already applied (a `Router<()>`), exactly like the spine's stateful routers,
/// so it merges cleanly.
///
/// A product also contributes its cloud tables through [`migrations`](BackendModule::migrations),
/// the cloud-side mirror of `local_store`'s `LocalModule::migrations`. It is
/// **opt-in**: the default is empty, so a product that adds only routes (or has
/// not added cloud tables yet) needs no override, and every existing
/// `BackendModule` impl keeps compiling unchanged. [`apply_module`] runs the
/// baseline schema and then the product's migrations against a [`PgExecutor`].
pub trait BackendModule {
    /// The module's stable identity and namespace.
    fn meta(&self) -> ProductModuleMeta;

    /// The product's backend routes, *without* the namespace prefix. [`mount`]
    /// nests them under `/<namespace>`.
    fn router(&self) -> Router;

    /// The product's cloud Postgres migrations, applied after the baseline by
    /// [`apply_module`]. By convention (see `docs/PRODUCT-MODULE-SEAM.md`) a
    /// product names its tables `<namespace>_<entity>` and its migration
    /// versions `<namespace>_NNNN_…`, so two products never collide in the shared
    /// `schema_migrations` ledger. Defaults to empty so this is purely additive.
    fn migrations(&self) -> &'static [PgMigration] {
        &[]
    }
}

/// Apply the baseline cloud schema and then `module`'s Postgres migrations to
/// `executor`, in order. Returns the total number of migrations applied.
///
/// Idempotent: running it twice applies the product's migrations only once,
/// because the runner records each version in `schema_migrations` and skips
/// already-applied versions. This is the cloud-side mirror of
/// `local_store::product_module::apply_module`, so a product's cloud schema is as
/// safe to re-apply as the spine's, and one product's run only ever emits the
/// versions in its own `migrations()` slice (never the baseline's beyond the
/// shared bootstrap, never another product's).
///
/// The namespace prefix is **enforced, not merely conventional**: every product
/// migration version must start with the module's `<namespace>_` prefix. The
/// `schema_migrations` ledger is keyed on the raw version string and shared by
/// every product, so an unprefixed or mis-prefixed version (say a product reusing
/// the baseline's `"0001_init"`, or another product's version) would be read as
/// already-applied and silently skipped, leaving the product's table uncreated
/// while the run still reported success. Rejecting it up front (before any SQL
/// runs) is what actually makes "two products never collide" true instead of
/// hoping authors remember the prefix.
pub fn apply_module<E, M>(executor: &mut E, module: &M) -> crate::pg_migration::Result<usize>
where
    E: PgExecutor + ?Sized,
    M: BackendModule + ?Sized,
{
    let prefix = module.meta().table_prefix();
    for m in module.migrations() {
        if !m.version.starts_with(&prefix) {
            return Err(PgMigrationError::Migration(format!(
                "product migration '{}' must start with the namespace prefix '{}' \
                 (the shared schema_migrations ledger is keyed on the raw version, \
                 so an unprefixed version collides with the baseline or another product)",
                m.version, prefix
            )));
        }
    }

    let baseline = run_migrations(executor, BASELINE_MIGRATIONS)?;
    let product = run_migrations(executor, module.migrations())?;
    Ok(baseline + product)
}

/// Nest a product module's router under its namespace prefix, yielding a router
/// ready to `.merge(...)` into the app router additively.
///
/// `axum::Router::nest` mounts the product's routes beneath `/<namespace>`, so a
/// product route `/records` becomes `/<namespace>/records`. Because the result
/// is a self-contained `Router`, merging it adds the product's routes without
/// altering any existing route — the additive-composition guarantee the seam
/// requires.
pub fn mount<M: BackendModule + ?Sized>(module: &M) -> Router {
    let prefix = module.meta().route_prefix();
    Router::new().nest(&prefix, module.router())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pg_migration::InMemoryPgExecutor;
    use axum::routing::get;

    /// A tiny in-test product module: one GET route under its namespace.
    struct TinyModule {
        namespace: &'static str,
    }

    impl BackendModule for TinyModule {
        fn meta(&self) -> ProductModuleMeta {
            ProductModuleMeta::new("Tiny", self.namespace).expect("valid namespace")
        }
        fn router(&self) -> Router {
            Router::new().route("/ping", get(|| async { "tiny-pong" }))
        }
    }

    /// An in-test product ("vault") that contributes one namespaced cloud table,
    /// mirroring the local-store `VaultLocal` test product. Its migration version
    /// carries the `vault_` prefix the convention requires.
    struct VaultBackend;

    const VAULT_PG_MIGRATIONS: &[PgMigration] = &[PgMigration {
        version: "vault_0001_init",
        sql: "CREATE TABLE vault_records (id TEXT PRIMARY KEY, body TEXT NOT NULL);",
    }];

    impl BackendModule for VaultBackend {
        fn meta(&self) -> ProductModuleMeta {
            ProductModuleMeta::new("Vault", "vault").expect("valid namespace")
        }
        fn router(&self) -> Router {
            Router::new().route("/records", get(|| async { "vault" }))
        }
        fn migrations(&self) -> &'static [PgMigration] {
            VAULT_PG_MIGRATIONS
        }
    }

    #[test]
    fn mount_uses_the_namespace_prefix_from_meta() {
        let module = TinyModule { namespace: "tiny" };
        // The prefix is derived from meta, not hardcoded at the route.
        assert_eq!(module.meta().route_prefix(), "/tiny");
        // `mount` returns a Router (smoke: it composes without panicking).
        let _router: Router = mount(&module);
    }

    // --- ISC-7 / ISC-24: a routes-only module contributes no migrations ---
    #[test]
    fn a_module_without_migrations_defaults_to_empty() {
        let module = TinyModule { namespace: "tiny" };
        assert!(
            module.migrations().is_empty(),
            "the default migrations() is empty, so existing modules are unchanged"
        );
    }

    // --- ISC-15: product migrations apply on top of the baseline ---
    #[test]
    fn applying_a_module_runs_baseline_then_product_migrations() {
        let mut exec = InMemoryPgExecutor::new();
        let applied = apply_module(&mut exec, &VaultBackend).unwrap();
        assert_eq!(
            applied,
            BASELINE_MIGRATIONS.len() + VAULT_PG_MIGRATIONS.len()
        );
    }

    // --- ISC-16: applying twice is idempotent ---
    #[test]
    fn applying_a_module_twice_is_idempotent() {
        let mut exec = InMemoryPgExecutor::new();
        apply_module(&mut exec, &VaultBackend).unwrap();
        let second = apply_module(&mut exec, &VaultBackend).unwrap();
        assert_eq!(second, 0, "second run applies nothing");
    }

    // --- ISC-17 / ISC-21: baseline versions apply before product versions, and
    // the runner emits only the baseline + this product's versions ---
    #[test]
    fn baseline_applies_before_product_and_nothing_else() {
        let mut exec = InMemoryPgExecutor::new();
        apply_module(&mut exec, &VaultBackend).unwrap();
        let emitted: Vec<&str> = exec.ran().iter().map(|(v, _)| v.as_str()).collect();
        let mut expected: Vec<&str> = BASELINE_MIGRATIONS.iter().map(|m| m.version).collect();
        expected.extend(VAULT_PG_MIGRATIONS.iter().map(|m| m.version));
        assert_eq!(
            emitted, expected,
            "baseline first, then product, nothing more"
        );
    }

    // --- ISC-18: the product's table/version carries the namespace prefix ---
    #[test]
    fn product_migration_uses_the_namespace_prefix() {
        let prefix = VaultBackend.meta().table_prefix();
        assert!(VAULT_PG_MIGRATIONS[0].version.starts_with(&prefix));
        assert!(VAULT_PG_MIGRATIONS[0]
            .sql
            .contains(&format!("{prefix}records")));
    }

    /// A misconfigured product whose migration version reuses the baseline's
    /// `"0001_init"` instead of carrying its `<namespace>_` prefix. Without the
    /// guard this would be silently skipped (the shared ledger already has
    /// `0001_init`) and `bad_records` would never be created.
    struct UnprefixedProduct;

    const UNPREFIXED_MIGRATIONS: &[PgMigration] = &[PgMigration {
        version: "0001_init",
        sql: "CREATE TABLE bad_records (id TEXT PRIMARY KEY);",
    }];

    impl BackendModule for UnprefixedProduct {
        fn meta(&self) -> ProductModuleMeta {
            ProductModuleMeta::new("Bad", "bad").expect("valid namespace")
        }
        fn router(&self) -> Router {
            Router::new()
        }
        fn migrations(&self) -> &'static [PgMigration] {
            UNPREFIXED_MIGRATIONS
        }
    }

    // --- ISC-38: a product version that collides with the baseline (no prefix)
    // is rejected before any SQL runs, not silently skipped (Forge HIGH) ---
    #[test]
    fn unprefixed_product_version_is_rejected() {
        let mut exec = InMemoryPgExecutor::new();
        match apply_module(&mut exec, &UnprefixedProduct) {
            Err(PgMigrationError::Migration(msg)) => {
                assert!(msg.contains("namespace prefix"), "got: {msg}");
            }
            other => panic!("expected a prefix-violation error, got {other:?}"),
        }
        // The guard runs before the baseline is even applied: nothing recorded.
        assert!(
            exec.applied_versions().is_empty(),
            "a rejected module applies nothing at all"
        );
    }
}
