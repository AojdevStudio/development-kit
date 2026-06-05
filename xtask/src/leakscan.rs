//! Leak scan: refuse to let a sensitive value reach the desktop tree (issue #24).
//!
//! ADR-0001/0002 say no Stripe/DB/webhook/signing secret may ship in the Tauri
//! app. The crate-edge check (`edges.rs`) proves the *capability* to reach those
//! secrets is absent from the desktop dependency graph; this check is the
//! complementary string-level backstop — it fails the gate if a *literal*
//! sensitive value is planted in desktop source or baked into the built debug
//! artifact, before a human review ever sees it.
//!
//! The matcher is a pure function over text/bytes so it is unit-testable without
//! touching the filesystem. The walker that feeds it desktop source files and
//! the compiled binary lives behind a thin adapter (`scan_desktop_tree`).

/// A sensitive-value category this stack must never ship to client machines.
/// Naming the category (rather than echoing the matched value) keeps the
/// secret out of CI logs while still telling the reviewer what was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// Stripe secret or restricted API key (`sk_live_`, `sk_test_`, `rk_live_`, `rk_test_`).
    StripeSecretKey,
    /// Stripe webhook signing secret (`whsec_`).
    StripeWebhookSecret,
    /// Postgres/SQL connection URL carrying credentials (`postgres://`, `postgresql://`).
    DatabaseUrl,
    /// PEM-encoded private key block (e.g. an ed25519 license-signing key).
    PrivateKey,
}

impl SecretKind {
    /// A short, log-safe label for the category.
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::StripeSecretKey => "Stripe secret/restricted key",
            SecretKind::StripeWebhookSecret => "Stripe webhook secret",
            SecretKind::DatabaseUrl => "database connection URL",
            SecretKind::PrivateKey => "private key",
        }
    }
}

/// One leak: which category was matched and which source it came from. The
/// matched value itself is deliberately *not* stored — a finding must be
/// reportable in CI without re-leaking the secret it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakFinding {
    pub kind: SecretKind,
    /// Where the leak was found (a file path, or the built-artifact label).
    pub source: String,
}

impl std::fmt::Display for LeakFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} found in {}", self.kind.label(), self.source)
    }
}

/// Scan a chunk of text for sensitive-value formats, tagging each finding with
/// `source`. Pure: no I/O, deterministic, so the pattern set is exercised by
/// fast unit tests.
pub fn scan_text(content: &str, source: &str) -> Vec<LeakFinding> {
    let mut found: Vec<SecretKind> = Vec::new();
    for rule in PREFIX_RULES {
        if !found.contains(&rule.kind) && content_has_prefixed_token(content, rule.prefix) {
            found.push(rule.kind);
        }
    }
    if !found.contains(&SecretKind::DatabaseUrl) && content_has_credentialed_db_url(content) {
        found.push(SecretKind::DatabaseUrl);
    }
    if content_has_pem_private_key(content) {
        found.push(SecretKind::PrivateKey);
    }
    found
        .into_iter()
        .map(|kind| LeakFinding {
            kind,
            source: source.to_string(),
        })
        .collect()
}

/// The minimum number of token-body characters that must follow a sensitive
/// prefix before we call it a real secret. A bare `sk_live_` in a doc comment or
/// a placeholder like `sk_live_<your-key>` carries no token body and must not
/// trip the gate; a planted real key always does.
const MIN_TOKEN_BODY: usize = 12;

/// A sensitive value identified by a literal prefix followed by a token body.
struct PrefixRule {
    kind: SecretKind,
    prefix: &'static str,
}

/// Prefix-shaped sensitive values for this stack. Stripe publishes these exact
/// prefixes; `whsec_` is the webhook signing secret.
const PREFIX_RULES: &[PrefixRule] = &[
    PrefixRule {
        kind: SecretKind::StripeSecretKey,
        prefix: "sk_live_",
    },
    PrefixRule {
        kind: SecretKind::StripeSecretKey,
        prefix: "sk_test_",
    },
    PrefixRule {
        kind: SecretKind::StripeSecretKey,
        prefix: "rk_live_",
    },
    PrefixRule {
        kind: SecretKind::StripeSecretKey,
        prefix: "rk_test_",
    },
    PrefixRule {
        kind: SecretKind::StripeWebhookSecret,
        prefix: "whsec_",
    },
];

/// Whether `content` contains `prefix` immediately followed by at least
/// `MIN_TOKEN_BODY` token characters (`[A-Za-z0-9]`). The token-body floor is
/// what separates a planted real secret from a prefix mentioned in prose.
fn content_has_prefixed_token(content: &str, prefix: &str) -> bool {
    content
        .match_indices(prefix)
        .any(|(idx, _)| token_body_len(&content[idx + prefix.len()..]) >= MIN_TOKEN_BODY)
}

/// The length of the leading run of token-body characters (`[A-Za-z0-9]`).
fn token_body_len(rest: &str) -> usize {
    rest.bytes()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count()
}

/// The SQL connection-URL schemes that, when carrying credentials, are a
/// database-credential leak on a client machine.
const DB_URL_SCHEMES: &[&str] = &["postgres://", "postgresql://"];

/// Whether `content` contains a database URL of the form
/// `scheme://user:pass@host`. We require the `user:pass@` userinfo so a
/// credential-less `postgres://localhost/db` mention in docs does not trip the
/// gate — the leak we care about is *credentials*, not the scheme.
fn content_has_credentialed_db_url(content: &str) -> bool {
    DB_URL_SCHEMES.iter().any(|scheme| {
        content
            .match_indices(scheme)
            .any(|(idx, _)| userinfo_has_credentials(&content[idx + scheme.len()..]))
    })
}

/// Given the text immediately after a URL scheme, whether the authority section
/// is `something:something@…` — i.e. a populated `user:password@host` userinfo.
fn userinfo_has_credentials(after_scheme: &str) -> bool {
    let Some(at) = after_scheme.find('@') else {
        return false;
    };
    let userinfo = &after_scheme[..at];
    match userinfo.split_once(':') {
        Some((user, pass)) => !user.is_empty() && !pass.is_empty(),
        None => false,
    }
}

/// Scan every scannable file under `root` (recursively) for sensitive values.
/// Binary and vendored directories are skipped; each remaining file is read as
/// UTF-8 (lossily) and run through [`scan_text`], tagged with its path.
///
/// Used for the desktop *source* tree. Unreadable files are skipped rather than
/// failing the scan — a leak scan that aborts on the first odd file is worse
/// than one that scans everything it can.
pub fn scan_directory(root: &std::path::Path) -> Vec<LeakFinding> {
    let mut findings = Vec::new();
    scan_directory_into(root, &mut findings);
    findings
}

/// Recursive helper for [`scan_directory`], accumulating into `out`.
fn scan_directory_into(dir: &std::path::Path, out: &mut Vec<LeakFinding>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if is_skippable_dir(&name) {
                continue;
            }
            scan_directory_into(&path, out);
        } else if is_scannable_file(&name) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.extend(scan_text(&content, &path.to_string_lossy()));
            }
        }
    }
}

/// Build/vendor directories that never hold authored source and would only slow
/// the scan (and risk byte-pattern false positives) if walked as text.
fn is_skippable_dir(name: &str) -> bool {
    matches!(name, "target" | "node_modules" | "dist" | ".git" | "icons")
}

/// Whether a file should be scanned as source text. We scan code, config, and
/// env-shaped files; binary assets (images, fonts, lockfiles' binaries) are not
/// authored secrets and are skipped to avoid noise.
fn is_scannable_file(name: &str) -> bool {
    const SCANNABLE_EXTS: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "json", "toml", "yaml", "yml", "html", "css", "env", "sh",
        "md", "txt", "pem", "key", "conf", "ini",
    ];
    // Dotfiles like `.env` and `.env.local` are scanned by name.
    if name.starts_with(".env") || name == ".npmrc" {
        return true;
    }
    match name.rsplit_once('.') {
        Some((_, ext)) => SCANNABLE_EXTS.contains(&ext),
        None => false,
    }
}

/// Render findings as a newline-separated report, one finding per line. Used by
/// the gate step to explain a failure. Never includes the matched value — only
/// the category and the source — so the report is safe to print in CI logs.
pub fn report_findings(findings: &[LeakFinding]) -> String {
    findings
        .iter()
        .map(LeakFinding::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scan a single built artifact (a compiled binary) for baked-in secrets. The
/// file is read as raw bytes and decoded lossily, because a leaked key embedded
/// in a binary is still ASCII even though the surrounding bytes are not.
pub fn scan_artifact(path: &std::path::Path) -> Vec<LeakFinding> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    scan_text(&text, &path.to_string_lossy())
}

/// Whether `content` contains a PEM private-key opening marker. All variants
/// (`PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `OPENSSH PRIVATE KEY`)
/// share the `-----BEGIN … PRIVATE KEY-----` envelope, so matching a `-----BEGIN`
/// header that ends in `PRIVATE KEY-----` covers them without a per-variant list.
fn content_has_pem_private_key(content: &str) -> bool {
    const HEADER: &str = "-----BEGIN ";
    const TRAILER: &str = "PRIVATE KEY-----";
    content.match_indices(HEADER).any(|(idx, _)| {
        content[idx + HEADER.len()..]
            .lines()
            .next()
            .is_some_and(|line| line.ends_with(TRAILER))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn kinds(findings: &[LeakFinding]) -> Vec<SecretKind> {
        findings.iter().map(|f| f.kind).collect()
    }

    /// A 24-char synthetic token body — enough to clear `MIN_TOKEN_BODY`.
    const SAMPLE_BODY: &str = "51HxYzABCdefGHIjklMNOpqr";

    /// Build a sample sensitive value from a `prefix` and a synthetic body *at
    /// runtime*. Assembling the token here (rather than writing the full literal
    /// in source) means no contiguous real-secret-shaped string ever appears in
    /// this file — so push-protection secret scanners do not flag the test
    /// fixtures — while `scan_text` still sees the fully assembled value and the
    /// detection path is exercised exactly as in production.
    fn sample(prefix: &str) -> String {
        format!("{prefix}{SAMPLE_BODY}")
    }

    /// A unique scratch directory under the OS temp dir, removed on drop, so the
    /// filesystem-walker tests need no external tempdir crate.
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
            let root = std::env::temp_dir().join(format!("xtask-leakscan-{tag}-{pid}-{nanos}"));
            std::fs::create_dir_all(&root).unwrap();
            Scratch { root }
        }

        /// Write `content` to `rel` under the scratch root, creating parent dirs.
        fn write(&self, rel: &str, content: &str) -> PathBuf {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn detects_a_stripe_live_secret_key() {
        // A representative Stripe live secret key shape: sk_live_ + token body,
        // assembled at runtime so the literal never lives in source.
        let content = format!(r#"let key = "{}";"#, sample("sk_live_"));
        let findings = scan_text(&content, "config.rs");
        assert_eq!(kinds(&findings), vec![SecretKind::StripeSecretKey]);
        assert_eq!(findings[0].source, "config.rs");
    }

    #[test]
    fn detects_stripe_test_and_restricted_keys() {
        // sk_test_, rk_live_, and rk_test_ are equally sensitive Stripe secrets.
        for prefix in ["sk_test_", "rk_live_", "rk_test_"] {
            let token = sample(prefix);
            let findings = scan_text(&token, "src.rs");
            assert_eq!(
                kinds(&findings),
                vec![SecretKind::StripeSecretKey],
                "prefix `{prefix}` should match as a Stripe secret key"
            );
        }
    }

    #[test]
    fn detects_a_stripe_webhook_secret() {
        let content = format!(r#"WEBHOOK_SECRET = "{}";"#, sample("whsec_"));
        let findings = scan_text(&content, "main.rs");
        assert_eq!(kinds(&findings), vec![SecretKind::StripeWebhookSecret]);
    }

    #[test]
    fn detects_a_postgres_url_with_credentials() {
        // A Postgres connection URL carrying a username:password is a database
        // credential leak — the desktop client must never hold one (ADR-0001).
        // Userinfo assembled at runtime so no literal credentialed URL is in source.
        for scheme in ["postgres://", "postgresql://"] {
            let url = format!("{scheme}app:{}@db.example.com:5432/prod", sample(""));
            let findings = scan_text(&url, "db.rs");
            assert_eq!(
                kinds(&findings),
                vec![SecretKind::DatabaseUrl],
                "scheme `{scheme}` with credentials should match as a database URL"
            );
        }
    }

    #[test]
    fn detects_a_pem_private_key_block() {
        // A license-signing private key would ship as a PEM block. Any
        // `-----BEGIN ... PRIVATE KEY-----` marker is a leak.
        for marker in [
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----BEGIN EC PRIVATE KEY-----",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        ] {
            let content = format!("{marker}\nMC4CAQAwBQYDK2Vw...\n-----END PRIVATE KEY-----");
            let findings = scan_text(&content, "key.rs");
            assert_eq!(
                kinds(&findings),
                vec![SecretKind::PrivateKey],
                "marker `{marker}` should match as a private key"
            );
        }
    }

    #[test]
    fn clean_content_yields_no_findings() {
        // Prose mentions of the prefixes, placeholders with no token body, and a
        // credential-less DB URL must NOT trip the gate — only planted real
        // values do. This is the false-positive floor that keeps a clean tree green.
        // `pk_live_` (publishable) is assembled at runtime to prove the matcher
        // ignores it without writing a key-shaped literal in source.
        let publishable = sample("pk_live_");
        let clean = format!(
            r#"
            // Set the Stripe secret key (sk_live_...) via an env var on the server.
            // Never ship sk_live_ keys in the desktop app.
            const PLACEHOLDER: &str = "whsec_<your-webhook-secret>";
            let local = "postgres://localhost:5432/devkit"; // no credentials
            // A comment that says PRIVATE KEY but is not a PEM block.
            let publishable = "{publishable}"; // publishable keys are not secret
        "#
        );
        let findings = scan_text(&clean, "clean.rs");
        assert!(
            findings.is_empty(),
            "clean content should yield no findings, got: {findings:?}"
        );
    }

    #[test]
    fn reports_every_distinct_category_in_mixed_content() {
        // A blob containing one of each category reports all four, deduplicated
        // per category, so one planted value never masks another. Sensitive
        // tokens are assembled at runtime to keep literals out of source.
        let content = format!(
            "{}\n{}\npostgresql://u:{}@host:5432/db\n-----BEGIN PRIVATE KEY-----\nMC4=\n-----END PRIVATE KEY-----\n",
            sample("sk_live_"),
            sample("whsec_"),
            sample(""),
        );
        let findings = scan_text(&content, "mixed.rs");
        let mut got = kinds(&findings);
        got.sort_by_key(|k| k.label());
        let mut want = vec![
            SecretKind::StripeSecretKey,
            SecretKind::StripeWebhookSecret,
            SecretKind::DatabaseUrl,
            SecretKind::PrivateKey,
        ];
        want.sort_by_key(|k| k.label());
        assert_eq!(got, want);
    }

    #[test]
    fn scan_directory_finds_a_planted_secret_in_a_nested_source_file() {
        let scratch = Scratch::new("dir-planted");
        scratch.write("src/clean.rs", "fn main() {}\n");
        scratch.write(
            "src/nested/config.rs",
            &format!(r#"const KEY: &str = "{}";"#, sample("sk_live_")),
        );

        let findings = scan_directory(&scratch.root);

        assert_eq!(kinds(&findings), vec![SecretKind::StripeSecretKey]);
        assert!(
            findings[0].source.ends_with("config.rs"),
            "finding names the offending file: {}",
            findings[0].source
        );
    }

    #[test]
    fn scan_directory_passes_a_clean_tree() {
        let scratch = Scratch::new("dir-clean");
        scratch.write("src/main.rs", "fn main() { println!(\"hi\"); }\n");
        scratch.write("src/App.tsx", "export const App = () => null;\n");
        scratch.write("Cargo.toml", "[package]\nname = \"x\"\n");

        assert!(scan_directory(&scratch.root).is_empty());
    }

    #[test]
    fn scan_directory_skips_build_output_directories() {
        let scratch = Scratch::new("dir-skip");
        // A secret baked into target/ is the *artifact's* concern, scanned
        // separately; walking target/ as source would be slow and noisy.
        scratch.write("target/debug/app", &sample("sk_live_"));
        scratch.write("src/main.rs", "fn main() {}\n");

        assert!(
            scan_directory(&scratch.root).is_empty(),
            "target/ must be skipped by the source walk"
        );
    }

    #[test]
    fn scan_artifact_finds_a_secret_baked_into_a_binary() {
        let scratch = Scratch::new("artifact");
        // Emulate a compiled binary: real machine-code bytes with a secret
        // string embedded, surrounded by non-UTF8 noise.
        let mut bytes: Vec<u8> = vec![0x00, 0xFF, 0x7F, 0x90, 0xC3];
        bytes.extend_from_slice(sample("sk_live_").as_bytes());
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let path = scratch.root.join("app-debug");
        std::fs::write(&path, &bytes).unwrap();

        let findings = scan_artifact(&path);

        assert_eq!(kinds(&findings), vec![SecretKind::StripeSecretKey]);
    }

    #[test]
    fn scan_artifact_is_clean_for_a_secret_free_binary() {
        let scratch = Scratch::new("artifact-clean");
        let bytes: Vec<u8> = vec![0x00, 0xFF, 0x7F, 0x90, 0xC3, 0xDE, 0xAD, 0xBE, 0xEF];
        let path = scratch.root.join("app-debug");
        std::fs::write(&path, &bytes).unwrap();

        assert!(scan_artifact(&path).is_empty());
    }

    #[test]
    fn scan_artifact_is_empty_for_a_missing_file() {
        // A not-yet-built artifact is handled by the gate step, not here; the
        // raw scanner simply reports nothing for a path that does not exist.
        let missing = std::env::temp_dir().join("xtask-leakscan-does-not-exist-xyz");
        assert!(scan_artifact(&missing).is_empty());
    }

    #[test]
    fn report_lists_every_finding_one_per_line() {
        let findings = vec![
            LeakFinding {
                kind: SecretKind::StripeSecretKey,
                source: "apps/desktop/src-tauri/src/lib.rs".to_string(),
            },
            LeakFinding {
                kind: SecretKind::PrivateKey,
                source: "target/debug/desktop".to_string(),
            },
        ];
        let report = report_findings(&findings);
        // Each finding appears on its own line, naming category + source, and
        // never echoes the secret value itself.
        assert!(report.contains("Stripe secret/restricted key"));
        assert!(report.contains("apps/desktop/src-tauri/src/lib.rs"));
        assert!(report.contains("private key"));
        assert!(report.contains("target/debug/desktop"));
        assert_eq!(report.lines().count(), 2, "one line per finding");
    }
}
