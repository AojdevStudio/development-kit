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
}
