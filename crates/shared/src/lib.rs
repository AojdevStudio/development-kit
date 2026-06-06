//! Pure shared types for the platform spine.
//!
//! This crate is the contract surface between the desktop app, the cloud API,
//! and the license capability crates. Per ADR-0002 it carries **types only** —
//! no `sqlx`, no Stripe, no secret loaders, no crypto. Keeping it dependency-thin
//! is what lets both the desktop tree and the backend depend on it without
//! dragging authority-bearing dependencies across the boundary.

#![forbid(unsafe_code)]

mod audit;
mod billing;
mod dto;
mod entitlements;
mod feature_key;
mod license;
mod plan;
mod product_entitlements;
mod product_feature_key;
mod product_module;
mod sync_queue;

pub use audit::{ActorKind, AuditEvent, Sensitivity};
pub use billing::{CheckoutSessionRequest, CheckoutSessionResponse, PortalSessionResponse};
pub use dto::{EntitlementsResponse, LicenseRefreshRequest, LicenseRefreshResponse};
pub use entitlements::{Entitlements, FeatureValue};
pub use feature_key::FeatureKey;
pub use license::LicenseToken;
pub use plan::{PlanTier, SubscriptionStatus};
pub use product_entitlements::ProductEntitlements;
pub use product_feature_key::{ProductFeatureKey, ProductFeatureKeyError, NAMESPACE_SEPARATOR};
pub use product_module::ProductModuleMeta;
pub use sync_queue::{
    ConflictPolicy, ConflictResolution, OpStatus, RetryDecision, RetryPolicy, SyncOperation,
    SyncQueue, SyncQueueStore,
};
