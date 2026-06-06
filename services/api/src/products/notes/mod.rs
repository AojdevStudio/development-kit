//! The Notes sample product — backend half (issue #37).
//!
//! Notes is the capstone sample product (issue #37): a minimal but real product
//! plugged into the kit through the documented seam (`docs/PRODUCT-MODULE-SEAM.md`)
//! ONLY — it contributes a [`BackendModule`](crate::product_module::BackendModule)
//! of cloud routes, a `LocalModule` of SQLite migrations (in `crates/local-store`),
//! a Tauri command (in `apps/desktop`), React screens, and a
//! [`ProductFeatureKey`](shared::ProductFeatureKey) registered for coverage. It
//! edits no shared-foundation enum, route chain, or migration list.
//!
//! Domain: a note-taking product. Creating and listing notes locally is FREE; the
//! one PAID capability is publishing a note to the cloud, gated by the
//! `notes.publish_note` product key. The paid gate is resolved server-side from
//! the caller's authenticated billing state (ADR-0001) by [`route`].

pub mod entitlement;
pub mod route;

use shared::ProductModuleMeta;

/// The Notes product namespace. Every dimension derives from it:
/// routes (`/notes/…`), feature keys (`notes.…`), tables (`notes_…`).
pub const NAMESPACE: &str = "notes";

/// The Notes product identity. Built through the validated constructor so the
/// namespace is provably `snake_case` (the seam rejects anything else).
pub fn meta() -> ProductModuleMeta {
    ProductModuleMeta::new("Notes", NAMESPACE).expect("valid notes namespace")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_meta_scopes_every_dimension_from_one_namespace() {
        let meta = meta();
        assert_eq!(meta.namespace, "notes");
        assert_eq!(meta.route_prefix(), "/notes");
        assert_eq!(meta.table_prefix(), "notes_");
    }
}
