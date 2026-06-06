//! Plugged-in product modules — desktop Tauri-command half (issue #37).
//!
//! Each product contributes its local Tauri command guards through the seam ONLY:
//! a command that enforces the product's paid keys against the server-resolved
//! [`ProductEntitlements`](shared::ProductEntitlements) snapshot (ADR-0001 — the
//! command is the local authority; React is UX only). No product edits the
//! desktop shell beyond registering its command in the `invoke_handler!` macro.

pub mod notes;
