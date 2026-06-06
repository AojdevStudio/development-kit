//! The gate runner library.
//!
//! `xtask` is the single entrypoint ADR-0002 requires: one binary that
//! orchestrates every check, called identically by local dev and CI. This
//! walking-skeleton version wires the checks that exist today (fmt, clippy,
//! workspace tests, the ADR-0002 edge check, and the Bun frontend slice) and is
//! structured so later issues bolt on cargo-deny, secret-scan, migrations,
//! webhook fixtures, and the feature-key coverage gate without reshaping it.

#![forbid(unsafe_code)]

pub mod bans;
pub mod coverage;
pub mod edges;
pub mod leakscan;
pub mod registry;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use cargo_metadata::MetadataCommand;

use crate::bans::{bans_check_failed, cargo_deny_args, config_gaps};
use crate::edges::{evaluate_edges, EdgeViolation, PackageDeps};

/// The desktop crate's manifest, relative to the workspace root. Rooting
/// cargo-deny here is what scopes the supply-chain bans to the desktop tree.
const DESKTOP_MANIFEST: &str = "apps/desktop/src-tauri/Cargo.toml";
/// The desktop crate's cargo-deny config, relative to the workspace root.
const DESKTOP_DENY_TOML: &str = "apps/desktop/src-tauri/deny.toml";

/// The workspace root: the parent of the xtask crate directory.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir)
}

/// Run the desktop supply-chain bans check (issue #23).
///
/// Two layers run here. First, a pure config-drift guard: the checked-in
/// `deny.toml` must still ban every crate the canonical Rust list requires — if
/// it has drifted, fail before shelling out, because cargo-deny would silently
/// enforce a weaker list. Second, `cargo deny check bans`, rooted at the desktop
/// manifest so only the desktop tree is analyzed and the api crate's legitimate
/// database/Stripe use is untouched.
///
/// A missing `cargo-deny` binary is a hard, actionable failure (not a skip): the
/// gate must not pass green just because the enforcer is absent in this env.
pub fn run_bans_check() -> Result<(), String> {
    let root = workspace_root();
    let deny_path = root.join(DESKTOP_DENY_TOML);

    // Layer 1 — config-drift guard (pure; never silently weakens the bans).
    let deny_toml = std::fs::read_to_string(&deny_path)
        .map_err(|e| format!("cannot read {}: {e}", deny_path.display()))?;
    let gaps = config_gaps(&deny_toml)?;
    if !gaps.is_empty() {
        return Err(format!(
            "{} no longer bans every required crate; missing: {}",
            deny_path.display(),
            gaps.join(", ")
        ));
    }

    // Layer 2 — cargo-deny over the desktop tree.
    run_cargo_deny_bans(&root)
}

/// Shell out to `cargo deny check bans` and interpret the result. Separated from
/// `run_bans_check` so the config-drift guard stays independently exercised.
fn run_cargo_deny_bans(root: &Path) -> Result<(), String> {
    let args = cargo_deny_args(DESKTOP_MANIFEST, DESKTOP_DENY_TOML);

    let output = Command::new("cargo")
        .args(&args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("failed to spawn `cargo deny`: {e}"))?;

    // `cargo deny` is a separate binary invoked as a cargo subcommand. If it is
    // not installed, cargo reports "no such command: `deny`". Turn that into an
    // actionable install hint rather than an opaque non-zero exit.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stderr.contains("no such command") {
        return Err(
            "cargo-deny is not installed; run `cargo install --locked cargo-deny` \
             (CI installs it via taiki-e/install-action)"
                .to_string(),
        );
    }

    let code = output.status.code().unwrap_or(-1);
    if bans_check_failed(code) {
        return Err(format!(
            "cargo-deny bans check failed (exit {code}) for the desktop tree:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            stderr
        ));
    }

    if !output.status.success() {
        // Non-bans failure (e.g. cargo-deny itself errored). Surface it.
        return Err(format!(
            "cargo-deny exited with {code} (not a bans violation):\n{stderr}"
        ));
    }

    Ok(())
}

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
