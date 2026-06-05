//! Integration test for the leak scan (issue #24).
//!
//! Acceptance criteria exercised here, through the public API only:
//!   - a planted sensitive value in the desktop *source* tree fails the scan,
//!   - a planted sensitive value baked into a *built artifact* fails the scan,
//!   - a clean tree + clean artifact passes.
//!
//! The scan's own pattern unit tests live next to the matcher; this file is the
//! end-to-end "plant a sample value, confirm the scan catches it" guarantee.

use std::path::{Path, PathBuf};

use xtask::leakscan::{report_findings, scan_artifact, scan_directory};

/// A 24-char synthetic token body — long enough to look like a real secret to
/// the scanner under test.
const SAMPLE_BODY: &str = "51HxYzABCdefGHIjklMNOpqr";

/// Build a sample sensitive value from `prefix` + a synthetic body *at runtime*.
/// Assembling the token here means no contiguous real-secret-shaped literal ever
/// appears in this file, so push-protection secret scanners do not flag the
/// fixtures — yet the scanner under test sees the fully assembled value.
fn sample(prefix: &str) -> String {
    format!("{prefix}{SAMPLE_BODY}")
}

/// A unique scratch directory removed on drop — no external tempdir crate.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("xtask-leak-it-{tag}-{pid}-{nanos}"));
        std::fs::create_dir_all(&root).unwrap();
        Scratch { root }
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Lay out a minimal desktop-shaped source tree mirroring `apps/desktop`.
fn scaffold_clean_desktop(scratch: &Scratch) {
    scratch.write(
        "src-tauri/src/lib.rs",
        "pub fn run() { /* product experience */ }\n",
    );
    scratch.write(
        "src-tauri/src/main.rs",
        "fn main() { desktop_lib::run(); }\n",
    );
    scratch.write("src/App.tsx", "export const App = () => null;\n");
    scratch.write("src-tauri/Cargo.toml", "[package]\nname = \"desktop\"\n");
}

#[test]
fn clean_desktop_tree_passes_the_scan() {
    let scratch = Scratch::new("clean");
    scaffold_clean_desktop(&scratch);

    let findings = scan_directory(scratch.root());

    assert!(
        findings.is_empty(),
        "a clean desktop tree must pass: {}",
        report_findings(&findings)
    );
}

#[test]
fn planted_stripe_secret_in_source_fails_the_scan() {
    let scratch = Scratch::new("stripe");
    scaffold_clean_desktop(&scratch);
    // Plant a Stripe live secret key in a Tauri command file (assembled at runtime).
    let planted = sample("sk_live_");
    scratch.write(
        "src-tauri/src/billing.rs",
        &format!(r#"const STRIPE: &str = "{planted}";"#),
    );

    let findings = scan_directory(scratch.root());

    assert!(
        !findings.is_empty(),
        "a planted Stripe secret must fail the scan"
    );
    let report = report_findings(&findings);
    assert!(report.contains("Stripe secret/restricted key"), "{report}");
    // The report must NOT echo the planted secret value.
    assert!(
        !report.contains(&planted),
        "report must not re-leak the secret: {report}"
    );
}

#[test]
fn planted_database_url_in_source_fails_the_scan() {
    let scratch = Scratch::new("dburl");
    scaffold_clean_desktop(&scratch);
    // Credentialed Postgres URL assembled at runtime so no literal DSN is in source.
    let dsn = format!("postgres://admin:{}@db.internal:5432/app", sample(""));
    scratch.write(
        "src-tauri/src/db.rs",
        &format!(r#"const DSN: &str = "{dsn}";"#),
    );

    let findings = scan_directory(scratch.root());

    assert!(
        report_findings(&findings).contains("database connection URL"),
        "a planted DB URL with credentials must fail the scan"
    );
}

#[test]
fn planted_signing_key_in_source_fails_the_scan() {
    let scratch = Scratch::new("pem");
    scaffold_clean_desktop(&scratch);
    scratch.write(
        "src-tauri/keys/signing.pem",
        "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIA==\n-----END PRIVATE KEY-----\n",
    );

    let findings = scan_directory(scratch.root());

    assert!(
        report_findings(&findings).contains("private key"),
        "a planted signing key must fail the scan"
    );
}

#[test]
fn planted_secret_baked_into_artifact_fails_the_scan() {
    let scratch = Scratch::new("artifact");
    // Emulate a compiled debug binary with a webhook secret baked in amongst
    // non-UTF8 machine-code bytes.
    let mut bytes: Vec<u8> = vec![0x7F, 0x45, 0x4C, 0x46, 0x00, 0xFF]; // ELF-ish header noise
    bytes.extend_from_slice(sample("whsec_").as_bytes());
    bytes.extend_from_slice(&[0xC3, 0x90, 0xDE, 0xAD]);
    let artifact = scratch.root().join("desktop-debug");
    std::fs::write(&artifact, &bytes).unwrap();

    let findings = scan_artifact(&artifact);

    assert!(
        report_findings(&findings).contains("Stripe webhook secret"),
        "a secret baked into the built artifact must fail the scan"
    );
}

#[test]
fn clean_artifact_passes_the_scan() {
    let scratch = Scratch::new("artifact-clean");
    let bytes: Vec<u8> = vec![0x7F, 0x45, 0x4C, 0x46, 0x00, 0xFF, 0xC3, 0x90, 0xDE, 0xAD];
    let artifact = scratch.root().join("desktop-debug");
    std::fs::write(&artifact, &bytes).unwrap();

    assert!(
        scan_artifact(&artifact).is_empty(),
        "a secret-free artifact must pass the scan"
    );
}
