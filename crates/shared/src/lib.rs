//! Pure shared types for the platform spine.
//!
//! This crate is the contract surface between the desktop app, the cloud API,
//! and the license capability crates. Per ADR-0002 it carries **types only** —
//! no `sqlx`, no Stripe, no secret loaders, no crypto. Keeping it dependency-thin
//! is what lets both the desktop tree and the backend depend on it without
//! dragging authority-bearing dependencies across the boundary.

#![forbid(unsafe_code)]

mod entitlements;
mod feature_key;
mod license;

pub use entitlements::{Entitlements, FeatureValue};
pub use feature_key::FeatureKey;
pub use license::LicenseToken;
