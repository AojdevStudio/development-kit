//! Product-module identity (issue #36 — product module seam).
//!
//! A product module contributes across two authority sides (ADR-0001/0002): a
//! backend axum `Router` on the cloud side and local SQLite migrations on the
//! desktop side. Those two sides must not pull each other's dependencies, so the
//! seam is split into two side-specific traits — `BackendModule` (in
//! `services/api`, names `axum::Router`) and `LocalModule` (in
//! `crates/local-store`, names its `Migration` type). What both sides share is
//! the module's **identity**: its stable id and namespace.
//!
//! [`ProductModuleMeta`] is that shared identity. It lives here in the
//! dependency-thin `shared` crate so both authority sides agree on the same
//! namespace string without either depending on the other. The namespace is the
//! single value that scopes a product's routes (`/<namespace>/…`), its feature
//! keys (`<namespace>.…`), and its table prefix (`<namespace>_…`) — so every
//! dimension of the seam derives from one place.

use serde::{Deserialize, Serialize};

/// The stable identity of a product module: a human id and the namespace that
/// scopes everything the product contributes.
///
/// The `namespace` MUST be `snake_case` (the same discipline product feature
/// keys follow) so it composes cleanly into route prefixes, table prefixes, and
/// `ProductFeatureKey` namespaces. Constructed through [`ProductModuleMeta::new`]
/// so an invalid namespace is rejected at the boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductModuleMeta {
    /// A stable identifier for the module, for logs and the audit trail.
    pub id: String,
    /// The namespace scoping the product's routes, feature keys, and tables.
    pub namespace: String,
}

impl ProductModuleMeta {
    /// Build module metadata, validating that `namespace` is non-empty
    /// `snake_case`. Returns `None` for an invalid namespace so a module can
    /// never register under a namespace that would break route/key/table scoping.
    pub fn new(id: impl Into<String>, namespace: impl Into<String>) -> Option<Self> {
        let namespace = namespace.into();
        if !is_snake_case(&namespace) {
            return None;
        }
        Some(ProductModuleMeta {
            id: id.into(),
            namespace,
        })
    }

    /// The route prefix this product mounts under: `/<namespace>`. Every product
    /// route is reachable beneath it, and the prefix derives from the namespace
    /// so it is declared in exactly one place.
    pub fn route_prefix(&self) -> String {
        format!("/{}", self.namespace)
    }

    /// The local/cloud table prefix convention for this product: `<namespace>_`.
    /// A product's tables are named `<namespace>_<entity>` so two products never
    /// collide and a reviewer can see at a glance which product owns a table.
    pub fn table_prefix(&self) -> String {
        format!("{}_", self.namespace)
    }
}

/// Whether `s` is non-empty `snake_case`: `[a-z][a-z0-9_]*`. Shared with the
/// product-feature-key discipline so a namespace that scopes keys is valid as a
/// key namespace too.
fn is_snake_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_constructs_with_a_snake_case_namespace() {
        let meta = ProductModuleMeta::new("Vault", "vault").expect("valid meta");
        assert_eq!(meta.namespace, "vault");
        assert_eq!(meta.id, "Vault");
    }

    #[test]
    fn meta_rejects_a_non_snake_case_namespace() {
        assert!(ProductModuleMeta::new("Vault", "Vault").is_none());
        assert!(ProductModuleMeta::new("Vault", "").is_none());
        assert!(ProductModuleMeta::new("Vault", "my vault").is_none());
    }

    #[test]
    fn route_prefix_is_slash_namespace() {
        let meta = ProductModuleMeta::new("Vault", "vault").unwrap();
        assert_eq!(meta.route_prefix(), "/vault");
    }

    #[test]
    fn table_prefix_is_namespace_underscore() {
        let meta = ProductModuleMeta::new("Vault", "vault").unwrap();
        assert_eq!(meta.table_prefix(), "vault_");
    }
}
