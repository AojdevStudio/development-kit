//! `cargo xtask gate [--scope <scope>]` — the single gate runner (ADR-0002).
//!
//! Scopes select a slice of the full gate. The walking skeleton implements the
//! checks that exist today; later issues extend `run_gate` with cargo-deny,
//! secret-scan, migrations, webhook fixtures, and the feature-key coverage gate
//! without changing how the gate is invoked.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use xtask::check_dependency_edges;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let command = args.first().map(String::as_str);
    match command {
        Some("gate") => {
            let scope = parse_scope(&args[1..]);
            run_gate(&scope)
        }
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
    eprintln!(
        "xtask — the platform-spine gate runner\n\n\
         USAGE:\n  \
         cargo xtask gate [--scope <scope>]\n  \
         cargo xtask edges\n\n\
         SCOPES:\n  \
         all       fmt + clippy + workspace tests + edges + frontend (default)\n  \
         rust      fmt + clippy + workspace tests + edges\n  \
         edges     ADR-0002 crate-dependency edge check only\n  \
         frontend  Bun lint + type-check + build (desktop frontend)\n  \
         desktop   desktop crate check + frontend\n  \
         api       api crate check + tests\n  \
         security  ADR-0002 edge check (compile-time authority boundary)\n"
    );
}

fn parse_scope(args: &[String]) -> String {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--scope" {
            if let Some(value) = iter.next() {
                return value.clone();
            }
        } else if let Some(value) = arg.strip_prefix("--scope=") {
            return value.to_string();
        }
    }
    "all".to_string()
}

/// A single named step in the gate. Steps fail loudly and the gate aggregates
/// the result so one red check does not mask others.
struct Step {
    name: &'static str,
    run: Box<dyn Fn() -> Result<(), String>>,
}

fn run_gate(scope: &str) -> ExitCode {
    let steps = steps_for_scope(scope);
    if steps.is_empty() {
        eprintln!("unknown scope: {scope}\n");
        print_help();
        return ExitCode::FAILURE;
    }

    let mut failures: Vec<String> = Vec::new();
    println!("== xtask gate (scope: {scope}) ==");
    for step in steps {
        println!("\n--> {}", step.name);
        match (step.run)() {
            Ok(()) => println!("    ok: {}", step.name),
            Err(e) => {
                eprintln!("    FAIL: {}: {e}", step.name);
                failures.push(step.name.to_string());
            }
        }
    }

    println!();
    if failures.is_empty() {
        println!("gate PASSED (scope: {scope})");
        ExitCode::SUCCESS
    } else {
        eprintln!("gate FAILED (scope: {scope}): {}", failures.join(", "));
        ExitCode::FAILURE
    }
}

fn steps_for_scope(scope: &str) -> Vec<Step> {
    let fmt = || Step {
        name: "cargo fmt --check",
        run: Box::new(|| cargo(&["fmt", "--all", "--", "--check"])),
    };
    let clippy = || Step {
        name: "cargo clippy -D warnings",
        run: Box::new(|| {
            cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])
        }),
    };
    let tests = || Step {
        name: "cargo test --workspace",
        run: Box::new(|| cargo(&["test", "--workspace"])),
    };
    let edges = || Step {
        name: "ADR-0002 crate-edge check",
        run: Box::new(edges_step),
    };
    let frontend = || Step {
        name: "frontend lint/type/build (bun)",
        run: Box::new(frontend_step),
    };

    match scope {
        "all" => vec![fmt(), clippy(), tests(), edges(), frontend()],
        "rust" => vec![fmt(), clippy(), tests(), edges()],
        "edges" | "security" => vec![edges()],
        "frontend" => vec![frontend()],
        "desktop" => vec![edges(), frontend()],
        "api" => vec![Step {
            name: "cargo test -p api",
            run: Box::new(|| cargo(&["test", "-p", "api"])),
        }],
        _ => vec![],
    }
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
