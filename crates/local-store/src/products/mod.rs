//! Plugged-in product modules — local/desktop half (issue #37).
//!
//! Each product contributes its local SQLite state through the seam's
//! [`LocalModule`](crate::product_module::LocalModule) ONLY: namespaced
//! `<namespace>_<entity>` tables added additively after the baseline schema. No
//! product edits the baseline [`MIGRATIONS`](crate::migration::MIGRATIONS) list.

pub mod notes;
