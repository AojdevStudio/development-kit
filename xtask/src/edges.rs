//! Mechanical ADR-0002 crate-edge enforcement.
//!
//! The authority split is a compile fact first (the desktop tree simply does
//! not list `license-sign`/`sqlx`/Stripe), and this check is the CI backstop:
//! it fails the gate the moment a forbidden edge is introduced, before a human
//! review ever sees it.
//!
//! The core is a pure function over a description of each package's *direct*
//! dependencies, so it is unit-testable without invoking cargo. The
//! `cargo_metadata` adapter that builds that description lives in `lib.rs`.

use std::collections::BTreeSet;

/// A package and the names of its direct dependencies.
#[derive(Debug, Clone)]
pub struct PackageDeps {
    pub name: String,
    pub direct_deps: BTreeSet<String>,
}

impl PackageDeps {
    pub fn new<I, S>(name: &str, deps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        PackageDeps {
            name: name.to_string(),
            direct_deps: deps.into_iter().map(Into::into).collect(),
        }
    }
}

/// A broken authority edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeViolation {
    pub package: String,
    pub dependency: String,
    pub reason: String,
}

impl std::fmt::Display for EdgeViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} depends on `{}`: {}",
            self.package, self.dependency, self.reason
        )
    }
}

/// A rule binding one package to its allowed capability crates and explicitly
/// banned dependencies. The desktop rule is the load-bearing one: it is what
/// makes "the client can never issue licenses or reach the database" mechanical.
struct EdgeRule {
    /// Workspace package this rule applies to.
    package: &'static str,
    /// Capability/authority crates this package is *forbidden* to depend on,
    /// each with the human reason for the ban.
    banned: &'static [(&'static str, &'static str)],
}

/// The ADR-0002 rule set. Keyed on the workspace package names in this repo.
const RULES: &[EdgeRule] = &[
    EdgeRule {
        package: "desktop",
        banned: &[
            (
                "license-sign",
                "desktop must verify licenses, never issue them (ADR-0002)",
            ),
            (
                "sqlx",
                "desktop must not reach Postgres; cloud authority only (ADR-0002)",
            ),
            (
                "tokio-postgres",
                "desktop must not reach Postgres; cloud authority only (ADR-0002)",
            ),
            (
                "async-stripe",
                "Stripe integration is backend-only (ADR-0002)",
            ),
            ("stripe", "Stripe integration is backend-only (ADR-0002)"),
        ],
    },
    EdgeRule {
        package: "api",
        banned: &[(
            "license-verify",
            "the backend issues licenses; it does not depend on the desktop verifier (ADR-0002)",
        )],
    },
];

/// The leaf util/types crates **explicitly exempted** from needing a crate-edge
/// rule. These carry no authority edge to constrain: `shared` is types-only
/// (ADR-0002), `xtask` is the gate runner, `local-store` is the desktop's local
/// SQLite (allowed on the client), and the `license-*` crates ARE the capability
/// crates the rules reference rather than constrain. Every OTHER workspace package
/// must have a rule.
///
/// **Default-deny by design (issue #59).** The rule-coverage check enumerates the
/// *actual* workspace packages (from `cargo metadata`) and requires a rule for
/// every one not on this exemption list — so a NEW package (e.g. a future
/// `mobile` client) is required to have a rule the moment it lands, rather than
/// being silently unconstrained because a hardcoded inclusion list never names it.
/// Exempting a new package is therefore a *conscious* edit to this list, reviewed
/// like any other authority decision.
///
/// This is the **direct-edge rule-coverage** complement to the `deny.toml`
/// transitive ban (`bans.rs`): edges proves each authority crate HAS a rule;
/// deny.toml proves no banned crate reaches the desktop transitively.
pub const RULE_EXEMPT_PACKAGES: &[&str] = &[
    "shared",
    "xtask",
    "local-store",
    "license-verify",
    "license-sign",
];

/// The workspace packages in `workspace_packages` that need a rule but have none.
///
/// Default-deny: a package needs a rule unless it is on [`RULE_EXEMPT_PACKAGES`].
/// A package "needs a rule and has none" is reported — that is exactly the
/// silent-unconstrained gap issue #59 flags. Pure over the package names + the
/// live `RULES`, so it is unit-testable; the caller supplies the real workspace
/// package set from `cargo metadata`, so a NEW crate is seen the moment it exists.
pub fn unconstrained_packages(workspace_packages: &[String]) -> Vec<String> {
    let ruled: BTreeSet<&str> = RULES.iter().map(|r| r.package).collect();
    let exempt: BTreeSet<&str> = RULE_EXEMPT_PACKAGES.iter().copied().collect();
    workspace_packages
        .iter()
        .filter(|pkg| !exempt.contains(pkg.as_str()) && !ruled.contains(pkg.as_str()))
        .cloned()
        .collect()
}

/// The rule-coverage assertion over the real workspace package set: every
/// non-exempt package must have an explicit edge rule, or the gate fails naming
/// the unconstrained package(s). `Ok(())` means no authority-bearing crate is
/// silently unconstrained.
///
/// `workspace_packages` is the live set from `cargo metadata` (built in `lib.rs`),
/// so this is default-deny against reality — a new client crate fails until it has
/// a rule or a conscious exemption.
pub fn evaluate_rule_coverage(workspace_packages: &[String]) -> Result<(), String> {
    let missing = unconstrained_packages(workspace_packages);
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} workspace package(s) have NO crate-edge rule and are not on the \
         exemption list — they are silently unconstrained (issue #59); add an \
         EdgeRule (or a conscious RULE_EXEMPT_PACKAGES entry) for each: {}",
        missing.len(),
        missing.join(", ")
    ))
}

/// Evaluate the ADR-0002 edges against the given package graph. Returns every
/// violation found; an empty vec means the crate graph honors the authority
/// split.
pub fn evaluate_edges(packages: &[PackageDeps]) -> Vec<EdgeViolation> {
    let mut violations = Vec::new();
    for rule in RULES {
        let Some(pkg) = packages.iter().find(|p| p.name == rule.package) else {
            // Package not present in the graph: nothing to check for it here.
            continue;
        };
        for (banned_dep, reason) in rule.banned {
            if pkg.direct_deps.contains(*banned_dep) {
                violations.push(EdgeViolation {
                    package: rule.package.to_string(),
                    dependency: (*banned_dep).to_string(),
                    reason: (*reason).to_string(),
                });
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_graph_has_no_violations() {
        let packages = vec![
            PackageDeps::new("desktop", ["shared", "license-verify", "tauri", "serde"]),
            PackageDeps::new("api", ["shared", "license-sign", "axum", "tokio"]),
        ];
        assert_eq!(evaluate_edges(&packages), vec![]);
    }

    #[test]
    fn desktop_depending_on_license_sign_is_a_violation() {
        let packages = vec![PackageDeps::new(
            "desktop",
            ["shared", "license-verify", "license-sign"],
        )];
        let violations = evaluate_edges(&packages);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].package, "desktop");
        assert_eq!(violations[0].dependency, "license-sign");
    }

    #[test]
    fn desktop_depending_on_sqlx_is_a_violation() {
        let packages = vec![PackageDeps::new("desktop", ["shared", "sqlx"])];
        let violations = evaluate_edges(&packages);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].dependency, "sqlx");
    }

    #[test]
    fn desktop_depending_on_stripe_is_a_violation() {
        let packages = vec![PackageDeps::new("desktop", ["shared", "async-stripe"])];
        let violations = evaluate_edges(&packages);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].dependency, "async-stripe");
    }

    #[test]
    fn api_depending_on_license_verify_is_a_violation() {
        let packages = vec![PackageDeps::new("api", ["shared", "license-verify"])];
        let violations = evaluate_edges(&packages);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].package, "api");
        assert_eq!(violations[0].dependency, "license-verify");
    }

    #[test]
    fn multiple_desktop_violations_are_all_reported() {
        let packages = vec![PackageDeps::new(
            "desktop",
            ["shared", "license-sign", "sqlx", "stripe"],
        )];
        let violations = evaluate_edges(&packages);
        assert_eq!(violations.len(), 3);
    }

    #[test]
    fn missing_package_is_not_a_violation() {
        // An empty graph should not panic or invent violations.
        assert_eq!(evaluate_edges(&[]), vec![]);
    }

    // ---- #59: rule-coverage assertion (default-deny: no package silently unconstrained) ----

    /// The real workspace package set this repo has today, as `cargo metadata`
    /// would report it. Used to exercise the default-deny check the way the live
    /// gate runs it.
    fn live_workspace_packages() -> Vec<String> {
        [
            "shared",
            "license-verify",
            "license-sign",
            "local-store",
            "api",
            "desktop",
            "xtask",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    // --- ISC-16 + ISC-19 (Anti): a NEW client crate in the workspace with no rule
    // and no exemption FAILS — the exact hardcoded-package bug, killed. ---
    #[test]
    fn a_new_unruled_workspace_package_is_flagged_unconstrained() {
        // Simulate a future `mobile` client crate landing with neither an EdgeRule
        // nor a conscious exemption. Default-deny means it MUST be reported — it is
        // not on a hardcoded inclusion list, it is simply "a package without a rule".
        let mut pkgs = live_workspace_packages();
        pkgs.push("mobile".to_string());
        let missing = unconstrained_packages(&pkgs);
        assert_eq!(
            missing,
            vec!["mobile".to_string()],
            "a new client crate with no rule must be flagged, not silently allowed"
        );
    }

    // --- ISC-13: the assertion fails (red) and names the unconstrained package ---
    #[test]
    fn evaluate_rule_coverage_fails_and_names_the_unconstrained_package() {
        let mut pkgs = live_workspace_packages();
        pkgs.push("mobile".to_string());
        let err =
            evaluate_rule_coverage(&pkgs).expect_err("an unconstrained package must fail the gate");
        assert!(err.contains("mobile"), "names the offender: {err}");
    }

    // --- ISC-14 + ISC-18: the LIVE workspace passes (both authority crates ruled,
    // every other crate consciously exempt) ---
    #[test]
    fn the_live_workspace_is_fully_rule_covered() {
        assert_eq!(
            unconstrained_packages(&live_workspace_packages()),
            Vec::<String>::new(),
            "every live non-exempt package must have an edge rule"
        );
        assert_eq!(evaluate_rule_coverage(&live_workspace_packages()), Ok(()));
    }

    // --- ISC-15: exempt leaf/util crates are NOT flagged (they carry no authority
    // edge); only authority-bearing crates need rules ---
    #[test]
    fn exempt_util_crates_are_not_flagged() {
        // A workspace of ONLY exempt crates yields no unconstrained packages.
        let exempt_only: Vec<String> = RULE_EXEMPT_PACKAGES.iter().map(|s| s.to_string()).collect();
        assert!(
            unconstrained_packages(&exempt_only).is_empty(),
            "exempt util/types crates must not be flagged"
        );
    }

    // --- ISC-17: consistency — every ruled package is real, and the exempt list
    // does not overlap the ruled list (a package is ruled XOR exempt, never both
    // by accident), so the direct-edge coverage is unambiguous and complements
    // deny.toml's transitive bans. ---
    #[test]
    fn ruled_and_exempt_sets_are_disjoint() {
        let ruled: BTreeSet<&str> = RULES.iter().map(|r| r.package).collect();
        let exempt: BTreeSet<&str> = RULE_EXEMPT_PACKAGES.iter().copied().collect();
        assert!(
            ruled.is_disjoint(&exempt),
            "a package must be either ruled or exempt, never both: {:?}",
            ruled.intersection(&exempt).collect::<Vec<_>>()
        );
    }

    // --- belt-and-suspenders: a package that IS ruled is never reported, even if
    // someone also listed it (defense against a future edit). ---
    #[test]
    fn a_ruled_package_is_never_reported_unconstrained() {
        assert!(unconstrained_packages(&["desktop".into(), "api".into()]).is_empty());
    }
}
