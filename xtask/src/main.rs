//! `cargo xtask gate [--scope <scope>]` — the single gate runner (ADR-0002).
//!
//! The gate is an *extensible check registry* (issue #22): every check is
//! registered once, tagged with the scopes it belongs to, and the runner
//! selects checks by scope. Adding cargo-deny, a secret scan, migration checks,
//! webhook fixtures, or the feature-key coverage gate later is a one-line
//! `register(...)` call in `build_registry` — the runner, the scope vocabulary,
//! and the CLI surface are untouched.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::registry::{Check, CheckRegistry, GateOutcome, Scope};
use xtask::{check_dependency_edges, run_bans_check};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let command = args.first().map(String::as_str);
    match command {
        Some("gate") => match parse_scope(&args[1..]) {
            Ok(scope) => run_gate(scope),
            Err(e) => {
                eprintln!("{e}\n");
                print_help();
                ExitCode::FAILURE
            }
        },
        Some("edges") => run_edges(),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    let scopes = Scope::ALL
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("|");
    eprintln!(
        "xtask — the platform-spine gate runner\n\n\
         USAGE:\n  \
         cargo xtask gate [--scope <scope>]\n  \
         cargo xtask edges\n\n\
         SCOPES: {scopes}\n  \
         all       every registered check (default)\n  \
         desktop   desktop crate edges + cargo-deny bans + frontend\n  \
         api       api crate tests\n  \
         frontend  Bun lint + type-check + build\n  \
         db        database/migration checks (registered by later issues)\n  \
         billing   Stripe/billing checks (registered by later issues)\n  \
         security  ADR-0002 edge check + cargo-deny desktop supply-chain bans\n  \
         prd       PRD intake / sample-product checks (registered by later issues)\n"
    );
}

/// Parse the `--scope <value>` (or `--scope=<value>`) flag, defaulting to
/// `all`. An unknown scope is a hard error so a typo never silently runs an
/// empty gate.
fn parse_scope(args: &[String]) -> Result<Scope, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--scope" {
            if let Some(value) = iter.next() {
                return Scope::parse(value);
            }
            return Err("--scope requires a value".to_string());
        } else if let Some(value) = arg.strip_prefix("--scope=") {
            return Scope::parse(value);
        }
    }
    Ok(Scope::All)
}

/// Run the gate for the selected scope. The runner is scope-agnostic: it builds
/// the registry, selects by scope, runs the slice, and aggregates failures.
fn run_gate(scope: Scope) -> ExitCode {
    let registry = build_registry();
    let selected = registry.select(scope);

    println!(
        "== xtask gate (scope: {}, {} check(s)) ==",
        scope.as_str(),
        selected.len()
    );
    for check in &selected {
        println!("  - {}", check.name);
    }

    let outcome = GateOutcome::run(&selected);
    for result in &outcome.results {
        match &result.outcome {
            Ok(()) => println!("\n--> {} ... ok", result.name),
            Err(e) => eprintln!("\n--> {} ... FAIL\n    {e}", result.name),
        }
    }

    println!();
    if outcome.passed() {
        println!("gate PASSED (scope: {})", scope.as_str());
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "gate FAILED (scope: {}): {}",
            scope.as_str(),
            outcome.failures().join(", ")
        );
        ExitCode::FAILURE
    }
}

/// Build the registry of every check the gate knows about. **This is the one
/// place a future issue adds a check** — register it with the scopes it belongs
/// to and the runner picks it up automatically.
///
/// Checks tagged with multiple scopes run under each of them (and always under
/// `all`). The Rust toolchain checks (fmt/clippy/tests) span the whole
/// workspace, so they belong to every Rust-bearing slice.
fn build_registry() -> CheckRegistry {
    let mut registry = CheckRegistry::new();

    registry
        .register(Check::new(
            "cargo fmt --check",
            [Scope::Desktop, Scope::Api, Scope::Security],
            Box::new(|| cargo(&["fmt", "--all", "--", "--check"])),
        ))
        .register(Check::new(
            "cargo clippy -D warnings",
            [Scope::Desktop, Scope::Api, Scope::Security],
            Box::new(|| {
                cargo(&[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ])
            }),
        ))
        .register(Check::new(
            "cargo test --workspace",
            [Scope::Desktop, Scope::Api],
            Box::new(|| cargo(&["test", "--workspace"])),
        ))
        .register(Check::new(
            "ADR-0002 crate-edge check",
            // The authority-boundary check is the heart of the `security` slice
            // and also guards the `desktop`/`api` boundaries directly.
            [Scope::Security, Scope::Desktop, Scope::Api],
            Box::new(edges_step),
        ))
        .register(Check::new(
            "frontend lint/type/build (bun)",
            [Scope::Frontend, Scope::Desktop],
            Box::new(frontend_step),
        ))
        .register(Check::new(
            "cargo-deny desktop bans (#23)",
            [Scope::Desktop, Scope::Security],
            Box::new(run_bans_check),
        ));

    registry
}

fn run_edges() -> ExitCode {
    match edges_step() {
        Ok(()) => {
            println!("ADR-0002 edges: ok");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ADR-0002 edges: FAIL\n{e}");
            ExitCode::FAILURE
        }
    }
}

fn edges_step() -> Result<(), String> {
    let violations = check_dependency_edges()?;
    if violations.is_empty() {
        Ok(())
    } else {
        let mut msg = String::from("authority-boundary violations:\n");
        for v in violations {
            msg.push_str(&format!("  - {v}\n"));
        }
        Err(msg)
    }
}

/// Run the frontend slice via Bun. Skips gracefully (with a clear message) if
/// the frontend has not been scaffolded yet, so the Rust gate stays runnable in
/// isolation.
fn frontend_step() -> Result<(), String> {
    let frontend_dir = workspace_root().join("apps/desktop");
    if !frontend_dir.join("package.json").exists() {
        return Err(format!(
            "no package.json under {} — frontend not scaffolded",
            frontend_dir.display()
        ));
    }
    run_in(&frontend_dir, "bun", &["install", "--frozen-lockfile"])
        .or_else(|_| run_in(&frontend_dir, "bun", &["install"]))?;
    run_in(&frontend_dir, "bun", &["run", "lint"])?;
    run_in(&frontend_dir, "bun", &["run", "typecheck"])?;
    run_in(&frontend_dir, "bun", &["run", "build"])?;
    Ok(())
}

fn cargo(args: &[&str]) -> Result<(), String> {
    run_in(&workspace_root(), env_cargo().as_str(), args)
}

fn env_cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn run_in(dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("failed to spawn `{program}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program} {}` exited with {}",
            args.join(" "),
            status
        ))
    }
}

/// The workspace root: the parent of the xtask crate dir.
fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at xtask/; the workspace root is its parent.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir)
}
