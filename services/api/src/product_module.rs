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

/// A product module's **backend** contribution: its identity and its cloud
/// routes.
///
/// Implementors return a `Router` of *un-prefixed* product routes (e.g.
/// `/records`); [`mount`] applies the `/<namespace>` prefix from the metadata, so
/// a route is never hardcoded with its namespace and the prefix is declared in
/// exactly one place. The router is returned as a `Router` with its own state
/// already applied (a `Router<()>`), exactly like the spine's stateful routers,
/// so it merges cleanly.
pub trait BackendModule {
    /// The module's stable identity and namespace.
    fn meta(&self) -> ProductModuleMeta;

    /// The product's backend routes, *without* the namespace prefix. [`mount`]
    /// nests them under `/<namespace>`.
    fn router(&self) -> Router;
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

    #[test]
    fn mount_uses_the_namespace_prefix_from_meta() {
        let module = TinyModule { namespace: "tiny" };
        // The prefix is derived from meta, not hardcoded at the route.
        assert_eq!(module.meta().route_prefix(), "/tiny");
        // `mount` returns a Router (smoke: it composes without panicking).
        let _router: Router = mount(&module);
    }
}
