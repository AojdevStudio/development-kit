//! The extensible check registry (issue #22).
//!
//! The gate is a *registry* of named checks, not a hard-coded list. Each check
//! declares which scopes it belongs to; the runner selects checks by scope and
//! never grows a per-scope `match` arm. A later issue plugs in cargo-deny, a
//! secret scan, migration checks, webhook fixtures, or the feature-key coverage
//! gate by pushing one `Check` onto the registry — the runner is untouched.
//!
//! Selection is a pure function over check metadata (name + scopes), so it is
//! unit-testable without spawning any subprocess. The side-effecting bodies
//! (which shell out to cargo/bun) live behind the `run` closure and are
//! exercised by the real `cargo xtask gate` invocation, not these unit tests.

use std::collections::BTreeSet;

/// The scopes a check can be tagged with. `Scope::All` is not a tag a check
/// carries; it is the *selector* meaning "every registered check".
///
/// Keeping scopes a closed enum (rather than free strings) makes an unknown
/// `--scope` a parse error at the boundary and makes "which scopes exist"
/// answerable from one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Desktop,
    Api,
    Frontend,
    Db,
    Billing,
    Security,
    Prd,
    /// Selector only: matches every registered check regardless of its tags.
    All,
}

impl Scope {
    /// The scopes accepted on the command line, in help/listing order. `All` is
    /// included because it is a valid `--scope` value (it selects everything).
    pub const ALL: &'static [Scope] = &[
        Scope::All,
        Scope::Desktop,
        Scope::Api,
        Scope::Frontend,
        Scope::Db,
        Scope::Billing,
        Scope::Security,
        Scope::Prd,
    ];

    /// The lowercase token used on the command line and in help text.
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Desktop => "desktop",
            Scope::Api => "api",
            Scope::Frontend => "frontend",
            Scope::Db => "db",
            Scope::Billing => "billing",
            Scope::Security => "security",
            Scope::Prd => "prd",
            Scope::All => "all",
        }
    }

    /// Parse a `--scope` token. Unknown tokens are rejected here so the runner
    /// never silently does nothing for a typo.
    pub fn parse(token: &str) -> Result<Scope, String> {
        Scope::ALL
            .iter()
            .copied()
            .find(|s| s.as_str() == token)
            .ok_or_else(|| {
                let known = Scope::ALL
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("|");
                format!("unknown scope `{token}` (expected one of: {known})")
            })
    }
}

/// One named step in the gate. `scopes` is the set of slices this check belongs
/// to; `run` performs the check and returns `Err(message)` on failure.
///
/// The runner only ever reads `name` and `scopes` to *select* checks — the
/// `run` body is opaque to selection, which is what keeps the registry
/// extensible without reshaping the runner.
pub struct Check {
    pub name: &'static str,
    pub scopes: BTreeSet<Scope>,
    pub run: Box<dyn Fn() -> Result<(), String>>,
}

impl Check {
    /// Build a check tagged with one or more scopes.
    pub fn new<I>(name: &'static str, scopes: I, run: Box<dyn Fn() -> Result<(), String>>) -> Self
    where
        I: IntoIterator<Item = Scope>,
    {
        Check {
            name,
            scopes: scopes.into_iter().collect(),
            run,
        }
    }

    /// Whether this check should run under the given selector.
    fn selected_by(&self, selector: Scope) -> bool {
        selector == Scope::All || self.scopes.contains(&selector)
    }
}

/// An ordered collection of checks. Registration order is preserved so the gate
/// reports checks in a stable, predictable sequence.
#[derive(Default)]
pub struct CheckRegistry {
    checks: Vec<Check>,
}

impl CheckRegistry {
    pub fn new() -> Self {
        CheckRegistry { checks: Vec::new() }
    }

    /// Register a check. This is the *only* call a future issue adds to wire a
    /// new check into the gate — the runner and the scope selector are untouched.
    pub fn register(&mut self, check: Check) -> &mut Self {
        self.checks.push(check);
        self
    }

    /// The checks that run under `selector`, in registration order.
    pub fn select(&self, selector: Scope) -> Vec<&Check> {
        self.checks
            .iter()
            .filter(|c| c.selected_by(selector))
            .collect()
    }

    /// Total number of registered checks (selector-independent).
    pub fn len(&self) -> usize {
        self.checks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }
}

/// The result of one check, captured so the gate can report every check and
/// aggregate failures without one red check masking the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub name: &'static str,
    pub outcome: Result<(), String>,
}

/// The aggregate result of running a slice of the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateOutcome {
    pub results: Vec<CheckResult>,
}

impl GateOutcome {
    /// Run every selected check, recording each result. Checks run to
    /// completion even after an earlier one fails — a single red check must not
    /// hide later ones from the report.
    pub fn run(checks: &[&Check]) -> GateOutcome {
        let results = checks
            .iter()
            .map(|c| CheckResult {
                name: c.name,
                outcome: (c.run)(),
            })
            .collect();
        GateOutcome { results }
    }

    /// The names of the checks that failed, in run order.
    pub fn failures(&self) -> Vec<&'static str> {
        self.results
            .iter()
            .filter(|r| r.outcome.is_err())
            .map(|r| r.name)
            .collect()
    }

    /// Whether the gate passed: every check succeeded.
    pub fn passed(&self) -> bool {
        self.results.iter().all(|r| r.outcome.is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A check whose body never runs in selection tests — selection reads only
    /// metadata, so the body is a placeholder.
    fn check(name: &'static str, scopes: &[Scope]) -> Check {
        Check::new(name, scopes.iter().copied(), Box::new(|| Ok(())))
    }

    fn names(checks: &[&Check]) -> Vec<&'static str> {
        checks.iter().map(|c| c.name).collect()
    }

    #[test]
    fn all_selector_runs_every_registered_check() {
        let mut registry = CheckRegistry::new();
        registry
            .register(check("fmt", &[Scope::Desktop, Scope::Api]))
            .register(check("frontend", &[Scope::Frontend]))
            .register(check("edges", &[Scope::Security]));

        assert_eq!(
            names(&registry.select(Scope::All)),
            vec!["fmt", "frontend", "edges"]
        );
    }

    #[test]
    fn a_named_scope_selects_only_its_slice() {
        let mut registry = CheckRegistry::new();
        registry
            .register(check("fmt", &[Scope::Desktop, Scope::Api]))
            .register(check("frontend", &[Scope::Frontend, Scope::Desktop]))
            .register(check("edges", &[Scope::Security]));

        // `--scope frontend` runs only the frontend-tagged checks.
        assert_eq!(names(&registry.select(Scope::Frontend)), vec!["frontend"]);
        // `--scope desktop` runs every check tagged desktop, in order.
        assert_eq!(
            names(&registry.select(Scope::Desktop)),
            vec!["fmt", "frontend"]
        );
    }

    #[test]
    fn a_scope_with_no_checks_selects_nothing() {
        let mut registry = CheckRegistry::new();
        registry.register(check("fmt", &[Scope::Desktop]));

        // `db` is a valid scope but no check is tagged with it yet — the gate
        // for that slice is simply empty, not an error.
        assert!(registry.select(Scope::Db).is_empty());
    }

    #[test]
    fn parse_accepts_every_documented_scope() {
        // The exact `--scope` vocabulary issue #22 specifies.
        let expected = [
            ("desktop", Scope::Desktop),
            ("api", Scope::Api),
            ("frontend", Scope::Frontend),
            ("db", Scope::Db),
            ("billing", Scope::Billing),
            ("security", Scope::Security),
            ("prd", Scope::Prd),
            ("all", Scope::All),
        ];
        for (token, scope) in expected {
            assert_eq!(Scope::parse(token), Ok(scope), "scope `{token}`");
        }
    }

    #[test]
    fn parse_rejects_an_unknown_scope() {
        let err = Scope::parse("nope").unwrap_err();
        assert!(err.contains("nope"), "error names the bad token: {err}");
        // The error lists the valid vocabulary so a typo is self-correcting.
        assert!(
            err.contains("desktop") && err.contains("prd"),
            "lists scopes: {err}"
        );
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for scope in Scope::ALL.iter().copied() {
            assert_eq!(Scope::parse(scope.as_str()), Ok(scope));
        }
    }

    fn passing(name: &'static str) -> Check {
        Check::new(name, [Scope::Api], Box::new(|| Ok(())))
    }

    fn failing(name: &'static str, why: &'static str) -> Check {
        Check::new(name, [Scope::Api], Box::new(move || Err(why.to_string())))
    }

    #[test]
    fn gate_passes_when_every_check_passes() {
        let checks = [passing("fmt"), passing("tests")];
        let selected: Vec<&Check> = checks.iter().collect();

        let outcome = GateOutcome::run(&selected);

        assert!(outcome.passed());
        assert!(outcome.failures().is_empty());
    }

    #[test]
    fn gate_fails_when_any_check_fails() {
        let checks = [passing("fmt"), failing("clippy", "1 warning")];
        let selected: Vec<&Check> = checks.iter().collect();

        let outcome = GateOutcome::run(&selected);

        assert!(!outcome.passed());
        assert_eq!(outcome.failures(), vec!["clippy"]);
    }

    #[test]
    fn one_red_check_does_not_mask_later_checks() {
        // fmt fails first, but tests must still run and also be reported failed.
        let checks = [
            failing("fmt", "needs formatting"),
            passing("clippy"),
            failing("tests", "2 failed"),
        ];
        let selected: Vec<&Check> = checks.iter().collect();

        let outcome = GateOutcome::run(&selected);

        assert!(!outcome.passed());
        assert_eq!(outcome.results.len(), 3, "every check ran");
        assert_eq!(outcome.failures(), vec!["fmt", "tests"]);
    }

    #[test]
    fn empty_selection_passes_vacuously() {
        // A scope with no checks is not a failure — it is an empty gate.
        let outcome = GateOutcome::run(&[]);
        assert!(outcome.passed());
        assert!(outcome.failures().is_empty());
    }
}
