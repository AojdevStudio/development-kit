//! Product-key derivation backstop (issue #59 — MEDIUM hardening).
//!
//! The feature-key coverage gate (`coverage.rs`) holds every *registered* product
//! key to a non-React-gate standard. But the registry itself is hand-edited: a
//! product author can declare a [`ProductFeatureKey`], gate it in a real backend
//! route via `require_product_feature(...)`, ship it — and if they never add it to
//! `product_key_registry()`, the coverage gate never knows the key exists and CI
//! stays green. That reintroduces default-allow-by-omission one level up: not "a
//! key with no gate test" (which `coverage.rs` already kills), but "a *gated* key
//! the gate never sees at all."
//!
//! This module is the structural backstop. It scans the product-seam **source**
//! for the call sites that DEFINE and ENFORCE product keys —
//! `ProductFeatureKey::new(<ns>, "<name>")` and the `require_product_feature(...)`
//! enforcement sites — derives the set of product keys that are actually gated in
//! the code, and asserts every one of them is present in the live
//! `product_key_registry()`. A route-gated key the author forgot to register is
//! therefore reported by name and fails the gate: "remembered to register" becomes
//! "mechanically forced to register."
//!
//! The core is pure functions over text + a key set, so the derivation logic is
//! unit-testable without spawning cargo or touching the filesystem. The thin
//! directory walker that feeds it the real seam source lives behind
//! [`scan_seam_dirs`], exercised by the live gate run and a focused test.

use std::collections::BTreeSet;
use std::path::Path;

use shared::ProductFeatureKey;

/// The seam-source directories the backstop scans, relative to the workspace
/// root. These are the only trees where a product *defines* (`ProductFeatureKey::
/// new`) and *enforces* (`require_product_feature`) its keys; the xtask registry
/// is deliberately excluded so the registry's own declarations are never mistaken
/// for gate sites (that would make the backstop tautological).
pub const SEAM_SOURCE_DIRS: &[&str] = &[
    "services/api/src/products",
    "apps/desktop/src-tauri/src/products",
];

/// Scan a single source file's text for the product keys it constructs.
///
/// Recognizes `ProductFeatureKey::new(<ns_expr>, "<name>")` where `<ns_expr>` is
/// either a literal string (`"notes"`) or an identifier the file binds to a
/// namespace — in this codebase the per-product `pub const NAMESPACE: &str =
/// "<ns>"` (and the `meta().namespace` derived from it). `parse(...)` and other
/// non-`new` constructions are intentionally NOT matched: `new` is the canonical
/// declaration site for a product's own keys, which is what the registry must
/// mirror.
///
/// Pure over the text, so the matcher is exercised by fast unit tests. The
/// per-file namespace is resolved from a `const NAMESPACE` binding in the same
/// text when the first argument is the `NAMESPACE` identifier (or `meta().
/// namespace`), so the real seam forms (`new(NAMESPACE, "publish_note")`,
/// `new(meta().namespace, "publish_note")`) resolve to the right key.
pub fn scan_text(content: &str, source: &str) -> Vec<ScannedKey> {
    scan_text_with_namespace(content, source, None)
}

/// Like [`scan_text`] but with a `dir_namespace` fallback resolved from the
/// product directory (the `const NAMESPACE` declared in a *sibling* file). This
/// closes a real fail-open: a key declared as `new(meta().namespace, "x")` in a
/// file that does NOT itself declare `const NAMESPACE` (e.g. the api
/// `entitlement.rs`, which references the const from the sibling `mod.rs`) would
/// otherwise be unresolvable and silently skipped — exactly the omission the
/// backstop exists to catch. Resolving against the directory's namespace fixes it.
pub fn scan_text_with_namespace(
    content: &str,
    source: &str,
    dir_namespace: Option<&str>,
) -> Vec<ScannedKey> {
    // Prefer a namespace declared in THIS file; fall back to the product
    // directory's namespace for sibling files that reference `meta().namespace`.
    let file_namespace = const_namespace(content).or_else(|| dir_namespace.map(str::to_string));
    let mut found = Vec::new();
    for (ns_expr, name) in new_call_args(content) {
        if let Some(namespace) = resolve_namespace(&ns_expr, file_namespace.as_deref()) {
            if let Ok(key) = ProductFeatureKey::new(&namespace, &name) {
                found.push(ScannedKey {
                    key,
                    source: source.to_string(),
                });
            }
        }
    }
    found
}

/// A product key found gated/declared in seam source, tagged with where it was
/// found so a failure can point the author at the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedKey {
    pub key: ProductFeatureKey,
    pub source: String,
}

/// The product keys present in `scanned` but absent from `registered`.
///
/// This is the proof core: a key the code gates (`scanned`) that the registry
/// (`registered`) never declares is exactly the silent escape the backstop kills.
/// The result preserves first-seen order and de-duplicates by key, so each
/// offending key is reported once with a stable message.
pub fn unregistered_gated_keys(
    scanned: &[ScannedKey],
    registered: &[ProductFeatureKey],
) -> Vec<ProductFeatureKey> {
    let registry: BTreeSet<&ProductFeatureKey> = registered.iter().collect();
    let mut seen: BTreeSet<ProductFeatureKey> = BTreeSet::new();
    let mut out = Vec::new();
    for hit in scanned {
        if !registry.contains(&hit.key) && seen.insert(hit.key.clone()) {
            out.push(hit.key.clone());
        }
    }
    out
}

/// Walk the seam-source directories under `root` and collect every product key
/// gated/declared there. The filesystem half of the backstop; pairs with
/// [`unregistered_gated_keys`] and the live registry in [`run_derivation_backstop`].
pub fn scan_seam_dirs(root: &Path) -> Vec<ScannedKey> {
    let mut found = Vec::new();
    for rel in SEAM_SOURCE_DIRS {
        let dir = root.join(rel);
        if dir.exists() {
            scan_dir_into(&dir, &mut found);
        }
    }
    found
}

/// Recursive helper for [`scan_seam_dirs`]: scan every `.rs` file in `dir`'s
/// subtree, resolving keys against the **product directory's** namespace so a key
/// declared with `meta().namespace` in a file that doesn't itself bind `const
/// NAMESPACE` (the sibling-file case) still resolves.
///
/// Two passes per directory: first find a `const NAMESPACE` anywhere in this
/// subtree (the product's namespace, declared once in its `mod.rs`), then scan
/// each file with that namespace as the fallback. A product subtree carries one
/// namespace, so a single discovered binding is the right fallback for its
/// siblings. Unreadable files are skipped rather than aborting the scan.
fn scan_dir_into(dir: &Path, out: &mut Vec<ScannedKey>) {
    let dir_namespace = discover_dir_namespace(dir);
    scan_dir_with_namespace(dir, dir_namespace.as_deref(), out);
}

/// Scan `dir`'s files (recursively) with `dir_namespace` as the namespace
/// fallback. A nested sub-product directory re-discovers its OWN namespace (so two
/// products under one parent don't bleed namespaces into each other).
fn scan_dir_with_namespace(dir: &Path, dir_namespace: Option<&str>, out: &mut Vec<ScannedKey>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Re-discover the namespace for the nested product subtree, falling
            // back to the parent's only if the child declares none.
            let child_ns =
                discover_dir_namespace(&path).or_else(|| dir_namespace.map(str::to_string));
            scan_dir_with_namespace(&path, child_ns.as_deref(), out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.extend(scan_text_with_namespace(
                    &content,
                    &path.to_string_lossy(),
                    dir_namespace,
                ));
            }
        }
    }
}

/// Find the `const NAMESPACE` declared anywhere directly in `dir`'s `.rs` files
/// (non-recursive: a product's namespace lives in its own `mod.rs`). Returns the
/// first one found, which is the product's namespace for its sibling files.
fn discover_dir_namespace(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Some(ns) = const_namespace(&content) {
                    return Some(ns);
                }
            }
        }
    }
    None
}

/// The `pub const NAMESPACE: &str = "<ns>"` binding in `content`, if present.
/// Every in-repo product module declares exactly one, and every dimension
/// (routes, tables, keys) derives from it — so resolving it once per file lets the
/// scanner turn `new(NAMESPACE, "x")` and `new(meta().namespace, "x")` into the
/// concrete key.
fn const_namespace(content: &str) -> Option<String> {
    const MARKER: &str = "const NAMESPACE";
    let idx = content.find(MARKER)?;
    let after = &content[idx + MARKER.len()..];
    let eq = after.find('=')?;
    string_literal(&after[eq + 1..])
}

/// The first double-quoted string literal at the start of `s` (after optional
/// whitespace), without quotes. Returns `None` if the next non-space token is not
/// a string literal.
fn string_literal(s: &str) -> Option<String> {
    let s = s.trim_start();
    let rest = s.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Resolve a `ProductFeatureKey::new` first-argument expression to a concrete
/// namespace. A literal (`"notes"`) resolves to itself; the `NAMESPACE` identifier
/// or a `meta().namespace` access resolves to the file's `const NAMESPACE`; any
/// other expression is unresolvable (returns `None`, so the key is skipped rather
/// than guessed).
fn resolve_namespace(ns_expr: &str, file_namespace: Option<&str>) -> Option<String> {
    let expr = ns_expr.trim();
    if let Some(lit) = string_literal(expr) {
        return Some(lit);
    }
    if expr == "NAMESPACE" || expr.contains("namespace") {
        return file_namespace.map(str::to_string);
    }
    None
}

/// Extract `(namespace_expr, name_literal)` pairs from every
/// `ProductFeatureKey::new(<ns_expr>, "<name>")` call in `content`. The name must
/// be a string literal (the canonical form for a declared key); the namespace
/// expression is returned verbatim for [`resolve_namespace`] to interpret.
fn new_call_args(content: &str) -> Vec<(String, String)> {
    const CALL: &str = "ProductFeatureKey::new(";
    let mut out = Vec::new();
    for (idx, _) in content.match_indices(CALL) {
        let after = &content[idx + CALL.len()..];
        let Some(args) = balanced_arg_list(after) else {
            continue;
        };
        // Split on the FIRST top-level comma so a namespace expression that itself
        // contains parens/commas (`meta().namespace`) is not truncated.
        let Some((ns_expr, name_arg)) = split_top_level_comma(args) else {
            continue;
        };
        if let Some(name) = string_literal(name_arg) {
            out.push((ns_expr.trim().to_string(), name));
        }
    }
    out
}

/// The substring of `after` up to (excluding) the `)` that closes the `new(` call,
/// honoring nested parentheses so `meta().namespace, "x"` is captured whole rather
/// than cut at the inner `)`. Returns `None` if the call is unterminated.
fn balanced_arg_list(after: &str) -> Option<&str> {
    let mut depth: usize = 0;
    for (i, ch) in after.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(&after[..i]),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Split an argument list on its first *top-level* comma (depth 0), so commas
/// inside a nested call (`meta(a, b).namespace, "x"`) do not split the namespace
/// from the name.
fn split_top_level_comma(args: &str) -> Option<(&str, &str)> {
    let mut depth: usize = 0;
    for (i, ch) in args.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some((&args[..i], &args[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Run the derivation backstop: scan the live seam source under `root`, diff
/// against the live `registered` keys, and fail (naming each offender) if any
/// gated key is unregistered. `Ok(())` means every key the code gates is declared
/// in the registry.
pub fn evaluate_derivation(root: &Path, registered: &[ProductFeatureKey]) -> Result<(), String> {
    let scanned = scan_seam_dirs(root);
    let missing = unregistered_gated_keys(&scanned, registered);
    if missing.is_empty() {
        return Ok(());
    }
    let names = missing
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "{} product feature key(s) are gated in seam source but absent from \
         product_key_registry() — a gated key MUST be registered or the coverage \
         gate cannot see it (issue #59): {names}",
        missing.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ns: &str, name: &str) -> ProductFeatureKey {
        ProductFeatureKey::new(ns, name).expect("valid product key")
    }

    // --- ISC-1: a literal-namespace new() call yields its key ---
    #[test]
    fn scans_a_literal_namespace_new_call() {
        let src = r#"let k = ProductFeatureKey::new("vault", "share_record").unwrap();"#;
        let found = scan_text(src, "vault.rs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, key("vault", "share_record"));
    }

    // --- ISC-2: the real notes form `new(NAMESPACE, "publish_note")` resolves ---
    #[test]
    fn resolves_the_const_namespace_form() {
        // Exactly the desktop/notes route shape: a per-file `const NAMESPACE` plus
        // `new(NAMESPACE, "name")`. The scanner must resolve the const to the key.
        let src = r#"
            pub const NAMESPACE: &str = "notes";
            pub fn publish_note_key() -> ProductFeatureKey {
                ProductFeatureKey::new(NAMESPACE, "publish_note").expect("valid")
            }
        "#;
        let found = scan_text(src, "notes/mod.rs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, key("notes", "publish_note"));
    }

    // --- ISC-2b: the `meta().namespace` form also resolves to the file namespace ---
    #[test]
    fn resolves_the_meta_namespace_form() {
        let src = r#"
            pub const NAMESPACE: &str = "notes";
            pub fn publish_note_key() -> ProductFeatureKey {
                ProductFeatureKey::new(meta().namespace, "publish_note").expect("valid")
            }
        "#;
        let found = scan_text(src, "notes/entitlement.rs");
        assert_eq!(
            found,
            vec![ScannedKey {
                key: key("notes", "publish_note"),
                source: "notes/entitlement.rs".into()
            }]
        );
    }

    // --- ISC-3: parse() and other non-new forms are NOT matched ---
    #[test]
    fn ignores_non_new_constructions() {
        let src = r#"
            let a = ProductFeatureKey::parse("vault.share_record").unwrap();
            let b = some_other_call("vault", "x");
        "#;
        assert!(scan_text(src, "x.rs").is_empty());
    }

    // --- ISC-4: unregistered_gated_keys reports the gap ---
    #[test]
    fn reports_a_gated_key_missing_from_the_registry() {
        let scanned = vec![
            ScannedKey {
                key: key("notes", "publish_note"),
                source: "a".into(),
            },
            ScannedKey {
                key: key("vault", "share_record"),
                source: "b".into(),
            },
        ];
        let registry = vec![key("notes", "publish_note")]; // vault NOT registered
        let missing = unregistered_gated_keys(&scanned, &registry);
        assert_eq!(missing, vec![key("vault", "share_record")]);
    }

    // --- ISC-5: empty when everything scanned is registered ---
    #[test]
    fn no_gap_when_every_scanned_key_is_registered() {
        let scanned = vec![ScannedKey {
            key: key("notes", "publish_note"),
            source: "a".into(),
        }];
        let registry = vec![key("notes", "publish_note"), key("vault", "share_record")];
        assert!(unregistered_gated_keys(&scanned, &registry).is_empty());
    }

    #[test]
    fn deduplicates_repeated_scanned_keys() {
        // The same key is constructed at several call sites (key fn, route, test);
        // it must be reported at most once.
        let scanned = vec![
            ScannedKey {
                key: key("vault", "share_record"),
                source: "a".into(),
            },
            ScannedKey {
                key: key("vault", "share_record"),
                source: "b".into(),
            },
        ];
        let missing = unregistered_gated_keys(&scanned, &[]);
        assert_eq!(missing, vec![key("vault", "share_record")]);
    }

    // --- ISC-6 + ISC-10: the LIVE seam + LIVE registry pass, AND the scan has a
    // positive lower bound (it actually extracts the real gated key from disk).
    //
    // The positive lower-bound is load-bearing: a scanner that matched NOTHING
    // would trivially satisfy "scanned ⊆ registry" (empty set) and provide ZERO
    // backstop while staying green. So this asserts the disk scan EXTRACTED the one
    // real plugged-in product key (`notes.publish_note`, declared in
    // `services/api/src/products/notes/entitlement.rs` via `new(meta().namespace,
    // …)` AND in the desktop `products/notes/mod.rs` via `new(NAMESPACE, …)` — two
    // different forms, both of which must resolve). `vault.share_record` is the
    // seam's *worked example* registered for coverage but NOT gated in a real
    // product route under `products/`, so it is intentionally not in the scanned
    // set — the scan boundary is "where real products live." ---
    #[test]
    fn the_live_seam_keys_are_all_registered() {
        let root = workspace_root();
        let registered: Vec<ProductFeatureKey> = crate::coverage::product_key_registry()
            .into_iter()
            .map(|e| e.key)
            .collect();
        let scanned = scan_seam_dirs(&root);
        // Positive lower bound: the scan is non-empty and contains the real key, so
        // green means "found + registered", never "matched nothing".
        assert!(
            !scanned.is_empty(),
            "the live seam scan must extract at least one real gated product key — \
             an empty scan would silently neuter the backstop"
        );
        assert!(
            scanned
                .iter()
                .any(|s| s.key == key("notes", "publish_note")),
            "the live scan must find notes.publish_note gated in the product source"
        );
        // And it resolves BOTH real seam forms: the desktop `new(NAMESPACE, …)` and
        // the api `new(meta().namespace, …)` both produce the same canonical key, so
        // at least two source files contribute the notes key.
        let notes_sources: Vec<&str> = scanned
            .iter()
            .filter(|s| s.key == key("notes", "publish_note"))
            .map(|s| s.source.as_str())
            .collect();
        assert!(
            notes_sources.iter().any(|s| s.contains("apps/desktop"))
                && notes_sources.iter().any(|s| s.contains("services/api")),
            "both the desktop (NAMESPACE) and api (meta().namespace) forms must \
             resolve to notes.publish_note; got sources: {notes_sources:?}"
        );
        assert_eq!(
            evaluate_derivation(&root, &registered),
            Ok(()),
            "every live gated key must be registered"
        );
    }

    // --- ISC-7 + ISC-12 (Anti): a gated-but-UNREGISTERED key FAILS, by name ---
    #[test]
    fn a_gated_but_unregistered_key_fails_the_backstop() {
        // Simulate the exact escape: notes.publish_note is gated in source but the
        // registry omits it (e.g. author forgot to register). The backstop must
        // report it by name and fail.
        let root = workspace_root();
        let registry_missing_notes: Vec<ProductFeatureKey> =
            crate::coverage::product_key_registry()
                .into_iter()
                .map(|e| e.key)
                .filter(|k| *k != key("notes", "publish_note"))
                .collect();
        let err = evaluate_derivation(&root, &registry_missing_notes)
            .expect_err("a gated-but-unregistered key must fail the backstop");
        assert!(
            err.contains("notes.publish_note"),
            "the offending key is named so the author knows what to fix: {err}"
        );
    }

    // --- ISC-11: the xtask registry's own new() calls are NOT scanned (the seam
    // dirs exclude xtask), so the registry declaration is never mistaken for a
    // gate site. Proven by construction: SEAM_SOURCE_DIRS lists only the backend +
    // desktop product trees. ---
    #[test]
    fn seam_dirs_exclude_the_xtask_registry() {
        assert!(
            SEAM_SOURCE_DIRS.iter().all(|d| !d.contains("xtask")),
            "the scan must never read xtask source, or the registry would self-satisfy"
        );
        assert!(SEAM_SOURCE_DIRS
            .iter()
            .any(|d| d.contains("services/api/src/products")));
        assert!(SEAM_SOURCE_DIRS.iter().any(|d| d.contains("apps/desktop")));
    }

    /// The workspace root: the parent of the xtask crate dir. Mirrors the helper
    /// in `lib.rs`/`main.rs` so the test resolves the real seam source from any cwd.
    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }
}
