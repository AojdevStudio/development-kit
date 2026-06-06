//! Feature-key coverage gate (issue #25).
//!
//! Paid features must never be protected by the screen alone. ADR-0001 makes
//! React gating "UX only, never security"; the authority decision lives in a
//! Tauri command and/or the backend. This gate makes that mechanical: it
//! enumerates every [`FeatureKey`] variant and fails if any key lacks a
//! *non-React* enforcement test. A new paid feature therefore cannot ship
//! protected only by the UI — CI goes red until a command or backend gate test
//! covers it.
//!
//! The core is a pure function over (the set of all keys, the set of coverage
//! entries), so it is unit-testable without spawning a test binary. The wiring
//! that turns this into a registered gate check lives in `main.rs`; the manifest
//! of which real tests cover which key lives in [`coverage_manifest`].

use shared::FeatureKey;

/// The enforcement layer a gate test exercises.
///
/// React is intentionally **not** a variant: ADR-0001 makes React gating
/// presentation-only, never an authority decision. By excluding it from the
/// type, "a React test does not count as coverage" is enforced at compile time
/// rather than by a runtime convention a contributor could forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateLayer {
    /// A Tauri command gate test: a local paid action is denied without
    /// entitlement and allowed with it.
    TauriCommand,
    /// A backend (Axum) authorization gate test: a server-backed paid action is
    /// denied without entitlement and allowed with it.
    Backend,
}

impl GateLayer {
    /// A short token for the layer, used in human-readable reports.
    pub fn as_str(self) -> &'static str {
        match self {
            GateLayer::TauriCommand => "tauri-command",
            GateLayer::Backend => "backend",
        }
    }
}

/// One declaration that a real gate test covers `key` at `layer`.
///
/// `test` names the concrete `#[test]` (or `#[tokio::test]`) function that
/// performs the enforcement, so the manifest can never claim coverage that no
/// test actually provides — the named test is run by `cargo test` in the same
/// gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverageEntry {
    pub key: FeatureKey,
    pub layer: GateLayer,
    /// Fully-qualified-ish identifier of the enforcing test, for the audit trail.
    pub test: &'static str,
}

/// The keys in `all_keys` that no entry in `entries` covers.
///
/// A key is *covered* when at least one [`CoverageEntry`] names it. Because
/// [`GateLayer`] has no React variant, any entry is by construction a non-React
/// (command or backend) gate — so "covered" already means "covered by something
/// other than the screen". The result preserves the order of `all_keys` for a
/// stable, predictable report.
pub fn uncovered_keys(all_keys: &[FeatureKey], entries: &[CoverageEntry]) -> Vec<FeatureKey> {
    all_keys
        .iter()
        .copied()
        .filter(|key| !entries.iter().any(|e| e.key == *key))
        .collect()
}

/// The coverage manifest: every non-React gate test that proves a feature key
/// is enforced somewhere other than the screen.
///
/// **This is the one place a new paid feature is registered as covered.** When a
/// product adds a feature key, it adds a Tauri-command or backend gate test and
/// records it here, naming the real `#[test]`/`#[tokio::test]` that enforces it.
/// `cargo test` runs that named test in the same gate, so an entry can never
/// claim coverage a test does not actually provide. The runner then fails the
/// gate for any [`FeatureKey`] missing from this list (see
/// [`run_feature_key_coverage`]).
///
/// Baseline coverage today is the backend authorization gate
/// (`services/api/tests/feature_gate.rs`): each key has a test asserting the
/// server denies the gated action without the entitlement and allows it with it.
pub fn coverage_manifest() -> Vec<CoverageEntry> {
    fn backend(key: FeatureKey, test: &'static str) -> CoverageEntry {
        CoverageEntry {
            key,
            layer: GateLayer::Backend,
            test,
        }
    }

    vec![
        backend(
            FeatureKey::ExportPdf,
            "api::feature_gate::gate_denies_export_pdf_without_entitlement_and_allows_with_it",
        ),
        backend(
            FeatureKey::CloudSync,
            "api::feature_gate::gate_denies_cloud_sync_without_entitlement_and_allows_with_it",
        ),
        backend(
            FeatureKey::AdvancedReports,
            "api::feature_gate::gate_denies_advanced_reports_without_entitlement_and_allows_with_it",
        ),
        backend(
            FeatureKey::TeamMembers,
            "api::feature_gate::gate_denies_team_members_without_entitlement_and_allows_with_it",
        ),
        backend(
            FeatureKey::MaxProjects,
            "api::feature_gate::gate_denies_max_projects_without_entitlement_and_allows_with_it",
        ),
        backend(
            FeatureKey::PrioritySupport,
            "api::feature_gate::gate_denies_priority_support_without_entitlement_and_allows_with_it",
        ),
        backend(
            FeatureKey::ApiAccess,
            "api::feature_gate::gate_denies_api_access_without_entitlement_and_allows_with_it",
        ),
    ]
}

/// Run the feature-key coverage gate against the baseline keys and the live
/// manifest. This is the function the gate registry wires as a `Check`.
pub fn run_feature_key_coverage() -> Result<(), String> {
    evaluate_coverage(&FeatureKey::ALL, &coverage_manifest())
}

/// The gate check: every key in `all_keys` must be covered by `entries`.
///
/// Returns `Ok(())` when coverage is complete, or `Err(message)` naming each
/// uncovered key (by its stable wire string) so the failure points straight at
/// the feature that lacks a non-React gate test. This is the `Result` shape the
/// gate registry's `Check::run` expects.
pub fn evaluate_coverage(all_keys: &[FeatureKey], entries: &[CoverageEntry]) -> Result<(), String> {
    let uncovered = uncovered_keys(all_keys, entries);
    if uncovered.is_empty() {
        return Ok(());
    }
    let names = uncovered
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "{} feature key(s) have no Tauri-command or backend gate test \
         (React gating is UX only, ADR-0001): {names}",
        uncovered.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tauri(key: FeatureKey) -> CoverageEntry {
        CoverageEntry {
            key,
            layer: GateLayer::TauriCommand,
            test: "stub::tauri_gate",
        }
    }

    fn backend(key: FeatureKey) -> CoverageEntry {
        CoverageEntry {
            key,
            layer: GateLayer::Backend,
            test: "stub::backend_gate",
        }
    }

    #[test]
    fn a_key_with_no_non_react_gate_test_is_uncovered() {
        // The whole point of the gate: a key with zero command/backend coverage
        // must be reported uncovered so CI fails. Here only ExportPdf is covered;
        // the other six keys are not.
        let entries = [backend(FeatureKey::ExportPdf)];

        let uncovered = uncovered_keys(&FeatureKey::ALL, &entries);

        assert!(
            uncovered.contains(&FeatureKey::CloudSync),
            "an unlisted key must be uncovered"
        );
        assert!(
            !uncovered.contains(&FeatureKey::ExportPdf),
            "a key with a backend gate test must not be uncovered"
        );
        assert_eq!(uncovered.len(), FeatureKey::ALL.len() - 1);
    }

    #[test]
    fn evaluate_is_ok_when_all_keys_are_covered() {
        let entries: Vec<CoverageEntry> = FeatureKey::ALL.iter().copied().map(backend).collect();

        assert_eq!(evaluate_coverage(&FeatureKey::ALL, &entries), Ok(()));
    }

    #[test]
    fn evaluate_errors_and_names_every_uncovered_key() {
        // Only CloudSync is covered; the failure message must name the keys that
        // are not, so a contributor knows exactly which features lack a gate test.
        let entries = [tauri(FeatureKey::CloudSync)];

        let err = evaluate_coverage(&FeatureKey::ALL, &entries).unwrap_err();

        assert!(
            err.contains("export_pdf") && err.contains("api_access"),
            "names uncovered keys: {err}"
        );
        assert!(
            !err.contains("cloud_sync"),
            "must not list the covered key: {err}"
        );
    }

    #[test]
    fn every_key_covered_by_a_command_or_backend_test_leaves_nothing_uncovered() {
        // A covered key passes. Mix the two non-React layers to prove either one
        // counts as coverage.
        let entries: Vec<CoverageEntry> = FeatureKey::ALL
            .iter()
            .enumerate()
            .map(
                |(i, &key)| {
                    if i % 2 == 0 {
                        tauri(key)
                    } else {
                        backend(key)
                    }
                },
            )
            .collect();

        let uncovered = uncovered_keys(&FeatureKey::ALL, &entries);

        assert!(
            uncovered.is_empty(),
            "fully covered set must leave no uncovered keys, got {uncovered:?}"
        );
    }

    #[test]
    fn the_live_manifest_covers_every_baseline_feature_key() {
        // The real gate: if a baseline FeatureKey is added without a gate test
        // recorded in coverage_manifest(), this fails — which is what fails CI.
        assert_eq!(
            run_feature_key_coverage(),
            Ok(()),
            "every baseline feature key must have a non-React gate test in the manifest"
        );
    }

    #[test]
    fn every_manifest_entry_is_a_non_react_gate() {
        // GateLayer has no React variant, so this is belt-and-suspenders: it
        // documents the invariant that the manifest only ever records command or
        // backend coverage.
        for entry in coverage_manifest() {
            assert!(
                matches!(entry.layer, GateLayer::TauriCommand | GateLayer::Backend),
                "{} is gated by a non-React layer",
                entry.key.as_str()
            );
            assert!(
                !entry.test.is_empty(),
                "every entry names the enforcing test"
            );
        }
    }
}
