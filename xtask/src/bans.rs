//! Supply-chain dependency bans for the desktop tree (issue #23).
//!
//! ADR-0001 splits authority between the client and the cloud; ADR-0002 makes
//! that split a compile fact via capability crates. The `edges` check guards the
//! *direct* `Cargo.toml` edges of workspace crates. This module is the
//! defence-in-depth supply-chain backstop: it bans the database and payment
//! client crate *families* from the desktop crate's **entire** dependency tree —
//! direct or transitive — so a banned crate can never reach the client even by
//! sneaking in behind another dependency.
//!
//! The enforcement that actually runs in CI is `cargo deny check bans`, rooted
//! at the desktop manifest so the api crate's legitimate use of `sqlx` / Stripe
//! is untouched. cargo-deny is an external binary, so the *running* of it lives
//! behind a subprocess body in the gate. Everything that can be a pure function —
//! the canonical ban list, the deny.toml/ban-list agreement check, the
//! exit-code interpretation, and the proof that a banned crate in the tree is
//! caught — lives here and is unit-tested without spawning anything.

use std::collections::BTreeSet;

/// A crate family the desktop tree must never contain, with the human reason for
/// the ban. The reason is what a contributor sees when the gate stops them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BannedCrate {
    /// The crate name as it appears on crates.io (and in `cargo metadata`).
    pub name: &'static str,
    /// Why the desktop tree may never contain it.
    pub reason: &'static str,
}

/// The canonical set of crates banned from the desktop dependency tree.
///
/// Two families, both flowing from the authority split (ADR-0001): database
/// client crates (the client never reaches Postgres) and payment/Stripe client
/// crates (billing is backend-only). The list is intentionally broader than the
/// `edges` rule — it names the whole client-crate family, including transitive
/// offenders, because cargo-deny walks the *entire* tree, not just direct edges.
pub const DESKTOP_BANNED: &[BannedCrate] = &[
    // --- Database client crates: desktop never reaches Postgres (ADR-0001). ---
    BannedCrate {
        name: "sqlx",
        reason: "desktop must not reach Postgres; cloud authority only (ADR-0001)",
    },
    BannedCrate {
        name: "sqlx-postgres",
        reason: "desktop must not reach Postgres; cloud authority only (ADR-0001)",
    },
    BannedCrate {
        name: "tokio-postgres",
        reason: "desktop must not reach Postgres; cloud authority only (ADR-0001)",
    },
    BannedCrate {
        name: "postgres",
        reason: "desktop must not reach Postgres; cloud authority only (ADR-0001)",
    },
    BannedCrate {
        name: "diesel",
        reason: "desktop must not embed a Postgres ORM; cloud authority only (ADR-0001)",
    },
    // --- Payment client crates: billing is backend-only (ADR-0001). ---
    BannedCrate {
        name: "async-stripe",
        reason: "Stripe integration is backend-only; no billing on the client (ADR-0001)",
    },
    BannedCrate {
        name: "stripe-rust",
        reason: "Stripe integration is backend-only; no billing on the client (ADR-0001)",
    },
    BannedCrate {
        name: "stripe",
        reason: "Stripe integration is backend-only; no billing on the client (ADR-0001)",
    },
    // The signing capability crate is cloud-only authority — never on the client.
    BannedCrate {
        name: "license-sign",
        reason: "desktop verifies licenses, never issues them (ADR-0001/ADR-0002)",
    },
];

/// The banned crate names, as a set, for membership tests.
pub fn banned_names() -> BTreeSet<&'static str> {
    DESKTOP_BANNED.iter().map(|b| b.name).collect()
}

/// A banned crate found in a dependency tree, with the reason it is banned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanViolation {
    pub crate_name: String,
    pub reason: String,
}

/// The proof core: given the *full* set of crate names in the desktop
/// dependency tree (direct + transitive, as `cargo deny`/`cargo metadata` would
/// see it), report every banned crate present.
///
/// An empty result means the tree honours the bans. A non-empty result is
/// exactly what makes "adding a banned dependency fails the gate" mechanical:
/// the offending crate appears here, the gate goes red, no human review needed.
pub fn tree_violations<I, S>(tree_crate_names: I) -> Vec<BanViolation>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let present: BTreeSet<String> = tree_crate_names
        .into_iter()
        .map(|s| s.as_ref().to_string())
        .collect();

    DESKTOP_BANNED
        .iter()
        .filter(|b| present.contains(b.name))
        .map(|b| BanViolation {
            crate_name: b.name.to_string(),
            reason: b.reason.to_string(),
        })
        .collect()
}

/// Extract the set of crate names denied by a cargo-deny `deny.toml`.
///
/// cargo-deny's `[bans].deny` array holds either bare strings (a crate name or a
/// PURL/package spec) or tables with a `name` key. We read the crate name out of
/// both forms; the leading `name` token of a spec string (before any version
/// requirement) is the crate name we ban on.
///
/// Returns `Err` with a human message if the input is not valid TOML — a
/// corrupt config must fail loudly, never silently parse to "bans nothing".
pub fn denied_crates_in_config(deny_toml: &str) -> Result<BTreeSet<String>, String> {
    let value: toml::Value =
        toml::from_str(deny_toml).map_err(|e| format!("invalid deny.toml: {e}"))?;

    let mut names = BTreeSet::new();
    let Some(deny) = value
        .get("bans")
        .and_then(|b| b.get("deny"))
        .and_then(|d| d.as_array())
    else {
        // No `[bans].deny` array at all: nothing denied. Not an error here —
        // `config_gaps` is what turns "denies nothing required" into a failure.
        return Ok(names);
    };

    for entry in deny {
        let name = match entry {
            toml::Value::String(spec) => spec_crate_name(spec),
            toml::Value::Table(table) => table.get("name").and_then(|n| n.as_str()),
            _ => None,
        };
        if let Some(name) = name {
            names.insert(name.to_string());
        }
    }
    Ok(names)
}

/// The crate name at the head of a cargo-deny package spec string, e.g.
/// `"sqlx"` from `"sqlx@0.7"` or `"sqlx:0.7"`. A bare name is returned as-is.
fn spec_crate_name(spec: &str) -> Option<&str> {
    spec.split(['@', ':']).next().filter(|s| !s.is_empty())
}

/// The canonical ban-list crates that the given `deny.toml` does **not** deny.
///
/// This is the config-drift guard: the checked-in cargo-deny config and the
/// canonical Rust ban list must agree, or the gate cannot trust cargo-deny to
/// enforce the full list. A non-empty result is a configuration bug, reported
/// before cargo-deny ever runs.
pub fn config_gaps(deny_toml: &str) -> Result<Vec<&'static str>, String> {
    let denied = denied_crates_in_config(deny_toml)?;
    Ok(DESKTOP_BANNED
        .iter()
        .map(|b| b.name)
        .filter(|name| !denied.contains(*name))
        .collect())
}

/// cargo-deny's `bans` check bit in its exit-code bitset. The exit code ORs one
/// bit per failed check: advisories `0x1`, bans `0x2`, licenses `0x4`,
/// sources `0x8`.
pub const BANS_EXIT_BIT: i32 = 0x2;

/// Whether a cargo-deny exit code indicates the *bans* check failed. We test the
/// specific bit rather than `code != 0` so the meaning stays precise even if a
/// future invocation runs more than one check.
pub fn bans_check_failed(exit_code: i32) -> bool {
    exit_code & BANS_EXIT_BIT != 0
}

/// Build the `cargo deny` argv that runs **only** the bans check, rooted at the
/// desktop manifest (which scopes the analyzed tree to the desktop crate) and
/// pointed at the desktop `deny.toml`.
pub fn cargo_deny_args(manifest_path: &str, config_path: &str) -> Vec<String> {
    vec![
        "deny".to_string(),
        "--manifest-path".to_string(),
        manifest_path.to_string(),
        "check".to_string(),
        "bans".to_string(),
        "--config".to_string(),
        config_path.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic "clean" desktop tree: the allowed capability crates plus the
    /// ordinary client-side crates Tauri pulls in. None are banned.
    fn clean_desktop_tree() -> Vec<&'static str> {
        vec![
            "desktop",
            "shared",
            "license-verify",
            "tauri",
            "serde",
            "serde_json",
            "ed25519-dalek",
            "rusqlite", // local SQLite is allowed on the client
        ]
    }

    #[test]
    fn a_clean_desktop_tree_has_no_ban_violations() {
        assert_eq!(tree_violations(clean_desktop_tree()), vec![]);
    }

    #[test]
    fn adding_sqlx_to_the_desktop_tree_is_a_violation() {
        // THE PROOF (acceptance criterion): a banned database client crate in
        // the desktop tree is caught — exactly what fails the gate.
        let mut tree = clean_desktop_tree();
        tree.push("sqlx");

        let violations = tree_violations(tree);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].crate_name, "sqlx");
        assert!(
            violations[0].reason.contains("Postgres"),
            "reason explains the ban: {}",
            violations[0].reason
        );
    }

    #[test]
    fn adding_a_payment_client_crate_to_the_desktop_tree_is_a_violation() {
        let mut tree = clean_desktop_tree();
        tree.push("async-stripe");

        let violations = tree_violations(tree);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].crate_name, "async-stripe");
    }

    #[test]
    fn a_transitively_pulled_banned_crate_is_still_caught() {
        // cargo-deny walks the whole tree, so a banned crate that arrives
        // transitively (not as a direct edge) is caught just the same.
        let tree = vec!["desktop", "shared", "some-wrapper", "tokio-postgres"];
        let violations = tree_violations(tree);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].crate_name, "tokio-postgres");
    }

    #[test]
    fn every_banned_family_member_is_reported() {
        // A pathological tree containing one of every banned crate reports them
        // all — one red crate never masks the others.
        let tree: Vec<&str> = DESKTOP_BANNED.iter().map(|b| b.name).collect();
        let violations = tree_violations(tree);
        assert_eq!(violations.len(), DESKTOP_BANNED.len());
    }

    #[test]
    fn parses_denied_crate_names_from_a_deny_toml() {
        let toml = r#"
            [bans]
            multiple-versions = "allow"
            deny = [
                { name = "sqlx", reason = "no postgres on the client" },
                { name = "async-stripe", reason = "billing is backend-only" },
            ]
        "#;
        let denied = denied_crates_in_config(toml).unwrap();
        assert!(denied.contains("sqlx"));
        assert!(denied.contains("async-stripe"));
        assert_eq!(denied.len(), 2);
    }

    #[test]
    fn config_parse_rejects_malformed_toml() {
        let err = denied_crates_in_config("this is { not valid").unwrap_err();
        assert!(!err.is_empty(), "a parse error message is returned");
    }

    #[test]
    fn config_gaps_reports_a_missing_ban() {
        // A config that bans only sqlx leaves every other family member
        // unguarded — those are the gaps.
        let toml = r#"
            [bans]
            deny = [{ name = "sqlx" }]
        "#;
        let gaps = config_gaps(toml).unwrap();
        assert!(gaps.contains(&"async-stripe"), "stripe is an unguarded gap");
        assert!(!gaps.contains(&"sqlx"), "sqlx is covered, not a gap");
    }

    #[test]
    fn a_config_covering_every_banned_crate_has_no_gaps() {
        let entries: Vec<String> = DESKTOP_BANNED
            .iter()
            .map(|b| format!("  {{ name = \"{}\" }},", b.name))
            .collect();
        let toml = format!("[bans]\ndeny = [\n{}\n]\n", entries.join("\n"));
        assert_eq!(config_gaps(&toml).unwrap(), Vec::<&'static str>::new());
    }

    /// The checked-in cargo-deny config for the desktop crate. Resolved relative
    /// to the xtask crate so the test runs from any working directory.
    fn checked_in_deny_toml() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("apps/desktop/src-tauri/deny.toml");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn the_checked_in_desktop_deny_toml_covers_the_whole_ban_list() {
        // The real artifact and the canonical list must agree — this is what
        // proves the deny.toml the gate runs actually bans every required crate.
        let gaps = config_gaps(&checked_in_deny_toml()).unwrap();
        assert_eq!(
            gaps,
            Vec::<&'static str>::new(),
            "deny.toml is missing bans for: {gaps:?}"
        );
    }

    #[test]
    fn cargo_deny_argv_roots_at_the_desktop_manifest_and_checks_only_bans() {
        let argv = cargo_deny_args("apps/desktop/src-tauri/Cargo.toml", "deny.toml");
        // Manifest-path rooting is what scopes the bans to the desktop tree.
        assert!(argv.contains(&"--manifest-path".to_string()));
        assert!(argv.contains(&"apps/desktop/src-tauri/Cargo.toml".to_string()));
        // Only the bans check runs — not advisories/licenses/sources.
        let check_idx = argv.iter().position(|a| a == "check").unwrap();
        assert_eq!(argv.get(check_idx + 1).map(String::as_str), Some("bans"));
        // The desktop config is selected explicitly.
        assert!(argv.contains(&"--config".to_string()));
        assert!(argv.contains(&"deny.toml".to_string()));
    }

    #[test]
    fn exit_code_zero_means_the_bans_check_passed() {
        assert!(!bans_check_failed(0));
    }

    #[test]
    fn the_bans_bit_in_the_exit_code_means_failure() {
        // cargo-deny's exit code is a bitset; 0x2 is the bans check.
        assert!(bans_check_failed(0x2));
        // bans failing alongside another check is still a bans failure.
        assert!(bans_check_failed(0x2 | 0x4));
    }

    #[test]
    fn an_exit_code_without_the_bans_bit_is_not_a_bans_failure() {
        // A licenses-only failure (0x4) must not be read as a bans failure;
        // the gate only runs `check bans`, but the interpretation stays precise.
        assert!(!bans_check_failed(0x4));
    }
}
