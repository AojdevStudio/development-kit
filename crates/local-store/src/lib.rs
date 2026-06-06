//! Local SQLite data service for the desktop app (ADR-0001/0002).
//!
//! Responsibilities:
//! - Apply all SQLite migrations in order on first open and on upgrades.
//! - Expose typed repositories that read/write local product state.
//!
//! Non-responsibilities (explicitly excluded by ADR-0001):
//! - No authoritative billing tables (subscriptions, Stripe data, entitlements).
//! - No signing keys or server secrets.

#![forbid(unsafe_code)]

pub mod db;
pub mod draft;
pub mod error;
pub mod migration;
pub mod product_module;
pub mod products;
