//! The gate runner library.
//!
//! `xtask` is the single entrypoint ADR-0002 requires: one binary that
//! orchestrates every check, called identically by local dev and CI. This
//! walking-skeleton version wires the checks that exist today (fmt, clippy,
//! workspace tests, the ADR-0002 edge check, and the Bun frontend slice) and is
//! structured so later issues bolt on cargo-deny, secret-scan, migrations,
//! webhook fixtures, and the feature-key coverage gate without reshaping it.

#![forbid(unsafe_code)]

pub mod edges;
pub mod leakscan;
pub mod registry;

use std::collections::BTreeSet;

use cargo_metadata::MetadataCommand;

use crate::edges::{evaluate_edges, EdgeViolation, PackageDeps};

/// Build the workspace package-dependency description from `cargo metadata` and
/// evaluate the ADR-0002 edges against it. Returns the list of violations.
///
/// Only *direct* dependencies of *workspace* packages are considered — that is
/// exactly the surface a contributor edits in a `Cargo.toml`.
pub fn check_dependency_edges() -> Result<Vec<EdgeViolation>, String> {
    let metadata = MetadataCommand::new()
        .exec()
        .map_err(|e| format!("failed to run `cargo metadata`: {e}"))?;

    let workspace_members: BTreeSet<_> = metadata.workspace_members.iter().cloned().collect();

    let packages: Vec<PackageDeps> = metadata
        .packages
        .iter()
        .filter(|p| workspace_members.contains(&p.id))
        .map(|p| {
            let deps = p
                .dependencies
                .iter()
                .map(|d| d.name.clone())
                .collect::<BTreeSet<_>>();
            PackageDeps {
                name: p.name.to_string(),
                direct_deps: deps,
            }
        })
        .collect();

    Ok(evaluate_edges(&packages))
}
