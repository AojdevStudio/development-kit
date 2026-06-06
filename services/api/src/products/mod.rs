//! Plugged-in product modules (issue #37).
//!
//! Each product lives in its own namespace submodule and plugs into the spine
//! ONLY through the seam (`docs/PRODUCT-MODULE-SEAM.md`): a
//! [`BackendModule`](crate::product_module::BackendModule) of routes, a server-side
//! product-entitlement policy, and a [`ProductFeatureKey`](shared::ProductFeatureKey)
//! registered for coverage. No product edits shared-foundation code.

pub mod notes;
